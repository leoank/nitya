//! Spike for EXP-20261123-bhfzfs: do calendar events round-trip without loss?
//!
//! The harness has its own small iCalendar reader (unfolding, content lines, components) and
//! expands recurrences with `rrule`, so what it measures does not depend on the crates it
//! compares. An event set's meaning is its *instances* over two years (start, duration,
//! summary, attendees, status, ...); two calendars mean the same when their instance sets
//! are equal. Raw properties are compared too, to name what a round trip drops or adds.
//!
//! Commands:
//!   crates <corpus> <out>          parse and re-serialize each case with each candidate crate
//!   caldav <collection> <corpus> <out>
//!                                  PUT each case to a CalDAV collection, GET it back, edit it,
//!                                  PUT it with If-Match, GET it again, compare each time
//!   show <file.ics>                print the instances the harness reads from one file
//!
//! Each prints one TSV row per case and leg, and one `===RESULT=== {json}` line at the end.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};

// ---------------------------------------------------------------- the reader

#[derive(Clone, Debug)]
struct Prop {
    name: String,
    params: BTreeMap<String, String>,
    value: String,
}

#[derive(Clone, Debug, Default)]
struct Comp {
    kind: String,
    props: Vec<Prop>,
    children: Vec<Comp>,
}

impl Comp {
    fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }
    fn text(&self, name: &str) -> String {
        self.get(name).map(|p| p.value.clone()).unwrap_or_default()
    }
}

fn unfold(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in s.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// Splits `NAME;P=V;Q="a:b":value`, respecting quoted parameter values.
fn content_line(line: &str) -> Prop {
    let mut in_quote = false;
    let mut cut = line.len();
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            ':' if !in_quote => {
                cut = i;
                break;
            }
            _ => {}
        }
    }
    let (head, value) = (&line[..cut], line.get(cut + 1..).unwrap_or(""));
    let mut parts = Vec::new();
    let (mut start, mut q) = (0, false);
    for (i, c) in head.char_indices() {
        match c {
            '"' => q = !q,
            ';' if !q => {
                parts.push(&head[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&head[start..]);
    let name = parts[0].to_ascii_uppercase();
    let params = parts[1..]
        .iter()
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Prop { name, params, value: value.to_string() }
}

fn parse(s: &str) -> Comp {
    let mut stack = vec![Comp { kind: "ROOT".into(), ..Default::default() }];
    for line in unfold(s) {
        let p = content_line(&line);
        match p.name.as_str() {
            "BEGIN" => stack.push(Comp { kind: p.value.to_ascii_uppercase(), ..Default::default() }),
            "END" if stack.len() > 1 => {
                let c = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(c);
            }
            _ => stack.last_mut().unwrap().props.push(p),
        }
    }
    while stack.len() > 1 {
        let c = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(c);
    }
    stack.pop().unwrap()
}

fn events(root: &Comp) -> Vec<Comp> {
    let mut out = Vec::new();
    fn walk(c: &Comp, out: &mut Vec<Comp>) {
        for ch in &c.children {
            if ch.kind == "VEVENT" {
                out.push(ch.clone());
            } else {
                walk(ch, out);
            }
        }
    }
    walk(root, &mut out);
    out
}

/// RFC 5545 folding: CRLF and at most 75 octets a line, never inside a UTF-8 sequence.
fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        let mut rest = line.as_str();
        let mut first = true;
        loop {
            let limit = if first { 75 } else { 74 };
            if rest.len() <= limit {
                if !first {
                    out.push(' ');
                }
                out.push_str(rest);
                out.push_str("\r\n");
                break;
            }
            let mut cut = limit;
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            if !first {
                out.push(' ');
            }
            out.push_str(&rest[..cut]);
            out.push_str("\r\n");
            rest = &rest[cut..];
            first = false;
        }
    }
    out
}

// ---------------------------------------------------------------- times

#[derive(Clone, Debug)]
enum When {
    Date(NaiveDate),
    Floating(NaiveDateTime),
    Utc(NaiveDateTime),
    Zoned(chrono_tz::Tz, NaiveDateTime),
}

fn when_values(p: &Prop) -> Vec<When> {
    p.value.split(',').filter_map(|v| when_one(v, p)).collect()
}

fn when_one(v: &str, p: &Prop) -> Option<When> {
    let v = v.trim();
    if p.params.get("VALUE").map(|s| s == "DATE").unwrap_or(false) || v.len() == 8 {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(When::Date);
    }
    if let Some(u) = v.strip_suffix('Z') {
        return NaiveDateTime::parse_from_str(u, "%Y%m%dT%H%M%S").ok().map(When::Utc);
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    match p.params.get("TZID") {
        Some(tz) => match tz.trim_start_matches('/').parse::<chrono_tz::Tz>() {
            Ok(z) => Some(When::Zoned(z, n)),
            Err(_) => Some(When::Floating(n)), // unknown zone: reported through the raw diff
        },
        None => Some(When::Floating(n)),
    }
}

impl When {
    /// The instant as rrule needs it; dates and floating times are pinned to UTC.
    fn rr(&self) -> DateTime<rrule::Tz> {
        match self {
            When::Date(d) => rrule::Tz::UTC.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()),
            When::Floating(n) | When::Utc(n) => rrule::Tz::UTC.from_utc_datetime(n),
            When::Zoned(z, n) => {
                let tz = rrule::Tz::Tz(*z);
                tz.from_local_datetime(n).earliest().unwrap_or_else(|| tz.from_utc_datetime(n))
            }
        }
    }
    /// Same kind and zone as `self`, at the instant `t` (for generated instances).
    fn like(&self, t: DateTime<rrule::Tz>) -> When {
        match self {
            When::Date(_) => When::Date(t.naive_utc().date()),
            When::Floating(_) => When::Floating(t.naive_utc()),
            When::Utc(_) => When::Utc(t.naive_utc()),
            When::Zoned(z, _) => When::Zoned(*z, t.with_timezone(z).naive_local()),
        }
    }
    /// The key a RECURRENCE-ID matches on: the UTC instant, the date, or the floating time.
    fn key(&self) -> String {
        match self {
            When::Date(d) => format!("D{}", d.format("%Y%m%d")),
            When::Floating(n) => format!("F{}", n.format("%Y%m%dT%H%M%S")),
            When::Utc(_) | When::Zoned(..) => {
                format!("U{}", self.rr().with_timezone(&Utc).format("%Y%m%dT%H%M%SZ"))
            }
        }
    }
    /// Shown form: kind, zone and local time, plus the UTC instant for zoned times.
    fn show(&self) -> String {
        match self {
            When::Date(d) => format!("date {}", d.format("%Y-%m-%d")),
            When::Floating(n) => format!("floating {}", n.format("%Y-%m-%dT%H:%M")),
            When::Utc(n) => format!("utc {}", n.format("%Y-%m-%dT%H:%M")),
            When::Zoned(z, n) => format!(
                "{} {} ({})",
                z.name(),
                n.format("%Y-%m-%dT%H:%M"),
                self.rr().with_timezone(&Utc).format("%H:%MZ")
            ),
        }
    }
    fn shift(&self, d: Duration) -> When {
        self.like(self.rr() + d)
    }
}

fn duration_of(ev: &Comp, start: &When) -> i64 {
    if let Some(end) = ev.get("DTEND").and_then(|p| when_values(p).into_iter().next()) {
        return (end.rr() - start.rr()).num_seconds();
    }
    if let Some(d) = ev.get("DURATION") {
        return parse_duration(&d.value);
    }
    if matches!(start, When::Date(_)) { 86_400 } else { 0 }
}

fn parse_duration(s: &str) -> i64 {
    let (sign, s) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.trim_start_matches('+')),
    };
    let (mut total, mut num) = (0i64, 0i64);
    for c in s.chars() {
        match c {
            '0'..='9' => num = num * 10 + (c as i64 - '0' as i64),
            'W' => { total += num * 604_800; num = 0 }
            'D' => { total += num * 86_400; num = 0 }
            'H' => { total += num * 3_600; num = 0 }
            'M' => { total += num * 60; num = 0 }
            'S' => { total += num; num = 0 }
            _ => {}
        }
    }
    sign * total
}

// ---------------------------------------------------------------- meaning

/// A TEXT value as a person reads it (RFC 5545 section 3.3.11).
fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' { out.push(c); continue; }
        match it.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(x) => out.push(x),
            None => out.push('\\'),
        }
    }
    out
}

/// The fields of an instance that a person would notice if they changed.
fn fields(ev: &Comp) -> String {
    let mut att: Vec<String> = ev
        .all("ATTENDEE")
        .map(|a| {
            format!(
                "{}={}/{}",
                a.value.to_ascii_lowercase().trim_start_matches("mailto:"),
                a.params.get("PARTSTAT").map(String::as_str).unwrap_or("NEEDS-ACTION"),
                a.params.get("ROLE").map(String::as_str).unwrap_or("REQ-PARTICIPANT"),
            )
        })
        .collect();
    att.sort();
    let mut alarms: Vec<String> = ev
        .children
        .iter()
        .filter(|c| c.kind == "VALARM")
        .map(|c| format!("{}{}", c.text("ACTION"), c.text("TRIGGER")))
        .collect();
    alarms.sort();
    let org = ev.text("ORGANIZER").to_ascii_lowercase();
    format!(
        "summary={:?} desc={:?} loc={:?} status={} transp={} class={} org={} att=[{}] alarms=[{}] url={} attach={:?} cat={}",
        unescape(&ev.text("SUMMARY")),
        unescape(&ev.text("DESCRIPTION")),
        unescape(&ev.text("LOCATION")),
        ev.get("STATUS").map(|p| p.value.as_str()).unwrap_or("CONFIRMED"),
        ev.get("TRANSP").map(|p| p.value.as_str()).unwrap_or("OPAQUE"),
        ev.get("CLASS").map(|p| p.value.as_str()).unwrap_or("PUBLIC"),
        org.trim_start_matches("mailto:"),
        att.join(","),
        alarms.join(","),
        ev.text("URL"),
        ev.all("ATTACH").map(|p| p.value.clone()).collect::<Vec<_>>(),
        ev.all("CATEGORIES").map(|p| p.value.clone()).collect::<Vec<_>>().join(","),
    )
}

/// Every instance between 2026-12-01 and 2029-01-01, one line each.
fn instances(root: &Comp) -> Result<BTreeSet<String>, String> {
    let from = rrule::Tz::UTC.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap();
    let to = rrule::Tz::UTC.with_ymd_and_hms(2029, 1, 1, 0, 0, 0).unwrap();
    let mut by_uid: BTreeMap<String, Vec<Comp>> = BTreeMap::new();
    for ev in events(root) {
        by_uid.entry(ev.text("UID")).or_default().push(ev);
    }
    let mut out = BTreeSet::new();
    for (uid, comps) in by_uid {
        let master = comps.iter().find(|c| c.get("RECURRENCE-ID").is_none());
        let overrides: Vec<&Comp> = comps.iter().filter(|c| c.get("RECURRENCE-ID").is_some()).collect();
        // (key, start, duration, fields) of each instance the master generates
        let mut insts: BTreeMap<String, (When, i64, String)> = BTreeMap::new();
        if let Some(m) = master {
            let start = m.get("DTSTART").and_then(|p| when_values(p).into_iter().next())
                .ok_or(format!("{uid}: no DTSTART"))?;
            let dur = duration_of(m, &start);
            let f = fields(m);
            let mut starts = vec![start.clone()];
            let has_rule = m.get("RRULE").is_some() || m.get("RDATE").is_some();
            if has_rule {
                let mut set = match m.get("RRULE") {
                    Some(r) => r.value.parse::<rrule::RRule<rrule::Unvalidated>>()
                        .map_err(|e| format!("{uid}: RRULE {e}"))?
                        .build(start.rr())
                        .map_err(|e| format!("{uid}: RRULE {e}"))?,
                    None => rrule::RRuleSet::new(start.rr()),
                };
                for p in m.all("RDATE") {
                    for w in when_values(p) { set = set.rdate(w.rr()); }
                }
                for p in m.all("EXDATE") {
                    for w in when_values(p) { set = set.exdate(w.rr()); }
                }
                let res = set.after(from).before(to).all(5000);
                starts = res.dates.into_iter().map(|t| start.like(t)).collect();
                if m.get("RRULE").is_none() && start.rr() >= from && start.rr() <= to
                    && !m.all("EXDATE").flat_map(when_values).any(|w| w.key() == start.key()) {
                    starts.push(start.clone());
                }
            }
            for s in starts {
                insts.insert(s.key(), (s, dur, f.clone()));
            }
        }
        // RANGE=THISANDFUTURE first (in order), then single-instance overrides
        let mut ovs = overrides.clone();
        ovs.sort_by_key(|o| o.get("RECURRENCE-ID").map(|p| p.params.contains_key("RANGE")).unwrap_or(false) == false);
        for o in ovs {
            let ridp = o.get("RECURRENCE-ID").unwrap();
            let rid = when_values(ridp).into_iter().next().ok_or(format!("{uid}: bad RECURRENCE-ID"))?;
            let ostart = o.get("DTSTART").and_then(|p| when_values(p).into_iter().next()).unwrap_or(rid.clone());
            let odur = duration_of(o, &ostart);
            let f = fields(o);
            if ridp.params.get("RANGE").map(|r| r == "THISANDFUTURE").unwrap_or(false) {
                let delta = ostart.rr() - rid.rr();
                let later: Vec<String> = insts.keys().filter(|k| **k >= rid.key()).cloned().collect();
                for k in later {
                    let (s, _, _) = insts.remove(&k).unwrap();
                    insts.insert(k, (s.shift(delta), odur, f.clone()));
                }
            } else {
                insts.insert(rid.key(), (ostart, odur, f));
            }
        }
        for (k, (s, d, f)) in insts {
            out.insert(format!("{uid} | rid {k} | {} | {d}s | {f}", s.show()));
        }
    }
    Ok(out)
}

/// Properties of each event, keyed by UID and RECURRENCE-ID, as `NAME;params:value` strings,
/// leaving out the ones every write may change.
fn raw_props(root: &Comp) -> BTreeSet<String> {
    const VOLATILE: [&str; 5] = ["DTSTAMP", "LAST-MODIFIED", "CREATED", "SEQUENCE", "PRODID"];
    let mut out = BTreeSet::new();
    let add = |scope: &str, c: &Comp, out: &mut BTreeSet<String>| {
        for p in &c.props {
            if VOLATILE.contains(&p.name.as_str()) { continue; }
            let params: Vec<String> = p.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            out.insert(format!("{scope} {};{}:{}", p.name, params.join(";"), p.value));
        }
    };
    for ev in events(root) {
        let scope = format!("{}#{}", ev.text("UID"), ev.text("RECURRENCE-ID"));
        add(&scope, &ev, &mut out);
        for (i, ch) in ev.children.iter().enumerate() {
            add(&format!("{scope}/{}{i}", ch.kind), ch, &mut out);
        }
    }
    // Time zones: their properties and their STANDARD and DAYLIGHT rules, by TZID.
    for cal in root.children.iter().filter(|c| c.kind == "VCALENDAR") {
        for tz in cal.children.iter().filter(|c| c.kind == "VTIMEZONE") {
            let scope = format!("VTIMEZONE:{}", tz.text("TZID"));
            add(&scope, tz, &mut out);
            for ch in &tz.children {
                add(&format!("{scope}/{}", ch.kind), ch, &mut out);
            }
        }
    }
    out
}

struct Diff {
    ok: bool,
    missing: Vec<String>,
    extra: Vec<String>,
    props_lost: BTreeSet<String>,
    props_added: BTreeSet<String>,
    error: Option<String>,
}

fn prop_name(s: &str) -> String {
    let after = s.split_once(' ').map(|x| x.1).unwrap_or(s);
    let name = after.split([';', ':']).next().unwrap_or(after);
    if s.starts_with("VTIMEZONE") { format!("VTIMEZONE.{name}") } else { name.to_string() }
}

fn compare(expected: &str, got: &str) -> Diff {
    let (a, b) = (parse(expected), parse(got));
    let (ia, ib) = match (instances(&a), instances(&b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => {
            return Diff { ok: false, missing: vec![], extra: vec![], props_lost: BTreeSet::new(),
                          props_added: BTreeSet::new(), error: Some(e) }
        }
    };
    let (ra, rb) = (raw_props(&a), raw_props(&b));
    Diff {
        ok: ia == ib,
        missing: ia.difference(&ib).cloned().collect(),
        extra: ib.difference(&ia).cloned().collect(),
        props_lost: ra.difference(&rb).map(|s| prop_name(s)).collect(),
        props_added: rb.difference(&ra).map(|s| prop_name(s)).collect(),
        error: None,
    }
}

fn row(case: &str, leg: &str, d: &Diff, detail: &mut String) -> String {
    let join = |s: &BTreeSet<String>| if s.is_empty() { "-".into() } else { s.iter().cloned().collect::<Vec<_>>().join(",") };
    if !d.ok || d.error.is_some() {
        detail.push_str(&format!("## {case} / {leg}\n"));
        if let Some(e) = &d.error { detail.push_str(&format!("error: {e}\n")); }
        for m in d.missing.iter().take(6) { detail.push_str(&format!("- {m}\n")); }
        for x in d.extra.iter().take(6) { detail.push_str(&format!("+ {x}\n")); }
        detail.push('\n');
    }
    format!(
        "{case}\t{leg}\t{}\t{}\t{}\t{}\t{}",
        if d.error.is_some() { "error" } else if d.ok { "same" } else { "CHANGED" },
        d.missing.len(),
        d.extra.len(),
        join(&d.props_lost),
        join(&d.props_added)
    )
}

fn corpus(dir: &Path) -> Vec<(String, String)> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path())
        .filter(|p| p.extension().map(|e| e == "ics").unwrap_or(false)).collect();
    v.sort();
    v.into_iter().map(|p| (p.file_stem().unwrap().to_string_lossy().into(), fs::read_to_string(&p).unwrap())).collect()
}

const HEADER: &str = "case\tleg\tmeaning\tinstances_missing\tinstances_extra\tprops_lost\tprops_added";

// ---------------------------------------------------------------- crates

fn run_crates(dir: &Path, out: &Path) {
    let candidates: [(&str, fn(&str) -> Result<String, String>); 3] = [
        ("icalendar", |s| s.parse::<icalendar::Calendar>().map(|c| c.to_string())),
        ("icalendar-parser", |s| {
            let u = icalendar::parser::unfold(s);
            icalendar::parser::read_calendar(&u).map(|c| c.to_string())
        }),
        ("calcard", |s| calcard::icalendar::ICalendar::parse(s).map(|c| c.to_string()).map_err(|e| format!("{e:?}"))),
    ];
    let mut detail = String::new();
    println!("{HEADER}");
    let mut same: BTreeMap<&str, usize> = BTreeMap::new();
    let cases = corpus(dir);
    for (name, text) in &cases {
        // the harness must read its own corpus without error
        instances(&parse(text)).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (cand, f) in &candidates {
            let d = match f(text) {
                Ok(s) => {
                    let p = out.join(cand);
                    fs::create_dir_all(&p).unwrap();
                    fs::write(p.join(format!("{name}.ics")), &s).unwrap();
                    compare(text, &s)
                }
                Err(e) => Diff { ok: false, missing: vec![], extra: vec![], props_lost: BTreeSet::new(),
                                 props_added: BTreeSet::new(), error: Some(format!("parse: {e}")) },
            };
            if d.ok && d.error.is_none() { *same.entry(cand).or_default() += 1; }
            println!("{}", row(name, cand, &d, &mut detail));
        }
    }
    fs::write(out.join("details.md"), &detail).unwrap();
    let res: Vec<String> = candidates.iter().map(|(c, _)| format!("\"{c}_same\": {}", same.get(c).unwrap_or(&0))).collect();
    println!("===RESULT=== {{\"cases\": {}, {}}}", cases.len(), res.join(", "));
}

// ---------------------------------------------------------------- caldav

/// The write-back edit nitya makes to every case: the summary of every component changed,
/// SEQUENCE bumped; plus a change of meaning in a few cases (a new EXDATE, an attendee's
/// answer, an override moved).
fn edit(name: &str, text: &str) -> String {
    let mut lines = unfold(text);
    let mut seq_seen = false;
    let mut out = Vec::new();
    for l in lines.drain(..) {
        if l.starts_with("SUMMARY") {
            out.push(format!("{l} (edited by nitya)"));
        } else if l.starts_with("SEQUENCE:") {
            let n: i64 = l[9..].trim().parse().unwrap_or(0);
            out.push(format!("SEQUENCE:{}", n + 1));
            seq_seen = true;
        } else if name.starts_with("07-") && l.starts_with("EXDATE") {
            out.push(format!("{l},20270216T140000"));
        } else if name.starts_with("18-") && l.contains("mailto:n@example.org") {
            out.push(l.replace("PARTSTAT=NEEDS-ACTION", "PARTSTAT=ACCEPTED"));
        } else if name.starts_with("08-") && l.starts_with("DTSTART;TZID=Europe/London:20270303T113000") {
            out.push("DTSTART;TZID=Europe/London:20270303T123000".into());
        } else if name.starts_with("08-") && l.starts_with("DTEND;TZID=Europe/London:20270303T120000") {
            out.push("DTEND;TZID=Europe/London:20270303T130000".into());
        } else {
            out.push(l);
        }
    }
    if !seq_seen {
        // a SEQUENCE on each VEVENT that lacked one
        let mut with = Vec::new();
        for l in out {
            let is_uid = l.starts_with("UID:");
            with.push(l);
            if is_uid { with.push("SEQUENCE:1".into()); }
        }
        out = with;
    }
    fold(&out)
}

struct Dav {
    auth: String,
}

impl Dav {
    fn req(&self, method: &str, url: &str) -> ureq::Request {
        ureq::request(method, url).set("Authorization", &self.auth)
    }
    fn send(r: ureq::Request, body: &str) -> Result<ureq::Response, String> {
        match r.send_string(body) {
            Ok(x) => Ok(x),
            Err(ureq::Error::Status(c, x)) => Err(format!("HTTP {c}: {}", x.into_string().unwrap_or_default().chars().take(300).collect::<String>())),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// One calendar per UID, each with every VTIMEZONE of the source: a CalDAV calendar object
/// resource holds one UID (RFC 4791 section 4.1), as nitya's store holds one item per UID.
fn split_by_uid(text: &str) -> Vec<String> {
    let lines = unfold(text);
    let (mut head, mut tzs, mut by_uid): (Vec<String>, Vec<String>, BTreeMap<String, Vec<String>>) =
        (Vec::new(), Vec::new(), BTreeMap::new());
    let (mut depth, mut cur, mut kind) = (0, Vec::new(), String::new());
    for l in lines {
        if depth == 0 && l.starts_with("BEGIN:") && l != "BEGIN:VCALENDAR" {
            kind = l[6..].to_string();
        }
        if l.starts_with("BEGIN:") && l != "BEGIN:VCALENDAR" { depth += 1; }
        if depth == 0 {
            if l != "END:VCALENDAR" && l != "BEGIN:VCALENDAR" { head.push(l); }
            continue;
        }
        cur.push(l.clone());
        if l.starts_with("END:") { depth -= 1; }
        if depth == 0 {
            if kind == "VEVENT" {
                let uid = cur.iter().find(|x| x.starts_with("UID:")).cloned().unwrap_or_default();
                by_uid.entry(uid).or_default().extend(cur.drain(..));
            } else {
                tzs.extend(cur.drain(..));
            }
        }
    }
    by_uid.into_values().map(|evs| {
        let mut all = vec!["BEGIN:VCALENDAR".to_string()];
        all.extend(head.iter().cloned());
        all.extend(tzs.iter().cloned());
        all.extend(evs);
        all.push("END:VCALENDAR".into());
        fold(&all)
    }).collect()
}

fn refusal(e: String) -> Diff {
    Diff { ok: false, missing: vec![], extra: vec![], props_lost: BTreeSet::new(), props_added: BTreeSet::new(), error: Some(e) }
}

fn run_caldav(collection: &str, dir: &Path, out: &Path) {
    // Radicale with `--auth-type none` takes any credentials; the user is "nitya".
    let dav = Dav { auth: std::env::var("CALDAV_AUTH").unwrap_or("Basic bml0eWE6eA==".into()) };
    let collection = collection.trim_end_matches('/').to_string() + "/";
    let mk = Dav::send(dav.req("MKCALENDAR", &collection), "");
    eprintln!("MKCALENDAR {collection}: {}", match &mk { Ok(r) => r.status().to_string(), Err(e) => e.clone() });
    fs::create_dir_all(out.join("got")).unwrap();
    fs::create_dir_all(out.join("edited")).unwrap();
    let mut detail = String::new();
    let (mut import_same, mut writeback_same, mut refused_whole) = (0, 0, 0);
    let ct = "text/calendar; charset=utf-8";
    println!("{HEADER}");
    let cases = corpus(dir);
    for (name, text) in &cases {
        let parts = split_by_uid(text);
        if parts.len() > 1 {
            // The case as one resource: expected to be refused, and recorded either way.
            let url = format!("{collection}{name}-whole.ics");
            let d = match Dav::send(dav.req("PUT", &url).set("Content-Type", ct).set("If-None-Match", "*"), text) {
                Err(e) => { refused_whole += 1; refusal(format!("PUT refused: {e}")) }
                Ok(_) => refusal("accepted several UIDs in one resource".into()),
            };
            println!("{}", row(name, "import(one resource)", &d, &mut detail));
        }
        let leg = |l: &str| if parts.len() > 1 { format!("{l}(split {})", parts.len()) } else { l.to_string() };
        let urls: Vec<String> = (0..parts.len())
            .map(|i| if parts.len() > 1 { format!("{collection}{name}-{i}.ics") } else { format!("{collection}{name}.ics") })
            .collect();
        let get = |dav: &Dav, url: &str| -> (String, String) {
            let r = Dav::send(dav.req("GET", url), "").expect("GET");
            let etag = r.header("ETag").unwrap_or("").to_string();
            (r.into_string().unwrap(), etag)
        };
        let mut failed = None;
        for (u, p) in urls.iter().zip(&parts) {
            if let Err(e) = Dav::send(dav.req("PUT", u).set("Content-Type", ct).set("If-None-Match", "*"), p) {
                failed = Some(format!("PUT refused: {e}"));
            }
        }
        if let Some(e) = failed {
            println!("{}", row(name, &leg("import"), &refusal(e), &mut detail));
            continue;
        }
        let got: Vec<(String, String)> = urls.iter().map(|u| get(&dav, u)).collect();
        let got_all: String = got.iter().map(|g| g.0.clone()).collect();
        fs::write(out.join("got").join(format!("{name}.ics")), &got_all).unwrap();
        let d = compare(text, &got_all);
        if d.ok && d.error.is_none() { import_same += 1; }
        println!("{}", row(name, &leg("import"), &d, &mut detail));

        // nitya edits what it stored (the server's copy) and writes it back with If-Match
        let mut edited_all = String::new();
        for (u, (g, etag)) in urls.iter().zip(&got) {
            let edited = edit(name, g);
            if let Err(e) = Dav::send(dav.req("PUT", u).set("Content-Type", ct).set("If-Match", etag), &edited) {
                failed = Some(format!("PUT edit refused: {e}"));
            }
            edited_all.push_str(&edited);
        }
        if let Some(e) = failed {
            println!("{}", row(name, &leg("write-back"), &refusal(e), &mut detail));
            continue;
        }
        let got2: String = urls.iter().map(|u| get(&dav, u).0).collect();
        fs::write(out.join("edited").join(format!("{name}.ics")), &got2).unwrap();
        let d2 = compare(&edited_all, &got2);
        // and the edit must have changed the meaning from the import
        let changed = !compare(&got_all, &got2).ok;
        if d2.ok && d2.error.is_none() && changed { writeback_same += 1; }
        println!("{}", row(name, &leg(if changed { "write-back" } else { "write-back(no-op!)" }), &d2, &mut detail));
    }
    fs::write(out.join("details.md"), &detail).unwrap();
    println!(
        "===RESULT=== {{\"cases\": {}, \"import_same\": {import_same}, \"writeback_same\": {writeback_same}, \"multi_uid_refused_as_one\": {refused_whole}}}",
        cases.len()
    );
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("crates") if a.len() == 4 => run_crates(Path::new(&a[2]), Path::new(&a[3])),
        Some("caldav") if a.len() == 5 => run_caldav(&a[2], Path::new(&a[3]), Path::new(&a[4])),
        Some("show") if a.len() == 3 => {
            for i in instances(&parse(&fs::read_to_string(&a[2]).unwrap())).unwrap() { println!("{i}"); }
        }
        _ => {
            eprintln!("usage: calendar-roundtrip crates <corpus> <out> | caldav <collection-url> <corpus> <out>");
            std::process::exit(2);
        }
    }
}
