"""Google Calendar and Microsoft Graph legs of EXP-20261123-bhfzfs.

    python3 providers.py google|graph|icloud <harness> <corpus> <out>

Each corpus case is written to a throwaway calendar named nitya-spike (created here,
deleted at the end), read back, converted to iCalendar and compared with the source by
the Rust harness (`<harness> compare`); then edited as nitya would (`<harness> edit`),
written back, read again and compared with the edit. The provider ids stay in memory, as
nitya's store would keep them; the source UID is used for what comes back, and whether
the provider kept the UID is counted.

Google and Graph speak JSON, so this file is also the experiment's first draft of those two
mappings (iCalendar to JSON and back). A case it cannot express is refused before any
write, with the reason, never written changed. Only what each API has a field for is
mapped; the rest is reported as lost by the harness.

Tokens: the owner's refresh tokens from nitya-consent, decrypted by agenix into
$NITYA_SECRETS (default ~/.config/nitya). Nothing secret is printed or written. The
owner's addresses and name are replaced by placeholders in everything written to <out>.

Graph cannot save a meeting without inviting its attendees, so case 18 is rewritten for
Graph: the owner's Microsoft account organizes, the owner's Google account is the one
attendee (the owner chose this). The Graph run then answers the invitation from Google
and waits for the answer to reach Graph (prediction 4). iCloud speaks CalDAV and stores
iCalendar as given; it also schedules on the server. The owner's Apple ID is their Gmail
address, the only one iCloud organizes as, so iCloud's case 18 invites the owner's
Microsoft account instead, and the run answers from Graph.
"""

import datetime as dt
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from zoneinfo import ZoneInfo

CAL_TZ = "Europe/London"
CAL_NAME = "nitya-spike"
WINDOW = ("2026-12-01T00:00:00Z", "2029-01-01T00:00:00Z")
SECRETS = os.path.expanduser(os.environ.get("NITYA_SECRETS", "~/.config/nitya"))
UTC = dt.timezone.utc


class Refused(Exception):
    """A case the provider cannot express; refused before writing."""


# ------------------------------------------------------------------ iCalendar


def unfold(text):
    out = []
    for raw in text.replace("\r\n", "\n").split("\n"):
        if raw[:1] in (" ", "\t") and out:
            out[-1] += raw[1:]
        elif raw:
            out.append(raw)
    return out


def content_line(line):
    q, cut = False, len(line)
    for i, c in enumerate(line):
        if c == '"':
            q = not q
        elif c == ":" and not q:
            cut = i
            break
    head, value = line[:cut], line[cut + 1:]
    parts, start, q = [], 0, False
    for i, c in enumerate(head):
        if c == '"':
            q = not q
        elif c == ";" and not q:
            parts.append(head[start:i])
            start = i + 1
    parts.append(head[start:])
    params = {}
    for p in parts[1:]:
        if "=" in p:
            k, v = p.split("=", 1)
            params[k.upper()] = v.strip('"')
    return {"name": parts[0].upper(), "params": params, "value": value, "line": line}


class Comp:
    def __init__(self, kind):
        self.kind, self.props, self.children = kind, [], []

    def get(self, name):
        return next((p for p in self.props if p["name"] == name), None)

    def all(self, name):
        return [p for p in self.props if p["name"] == name]

    def text(self, name):
        p = self.get(name)
        return unescape(p["value"]) if p else ""


def parse(text):
    stack = [Comp("ROOT")]
    for line in unfold(text):
        p = content_line(line)
        if p["name"] == "BEGIN":
            stack.append(Comp(p["value"].upper()))
            stack[-1].lines = [line]
        elif p["name"] == "END" and len(stack) > 1:
            c = stack.pop()
            c.lines.append(line)
            stack[-1].children.append(c)
            if hasattr(stack[-1], "lines"):
                stack[-1].lines.extend(c.lines)
        else:
            stack[-1].props.append(p)
            if hasattr(stack[-1], "lines"):
                stack[-1].lines.append(line)
    return stack[0]


def vcal(root):
    return next(c for c in root.children if c.kind == "VCALENDAR")


def by_uid(cal):
    """{uid: (master or None, [overrides])} in source order."""
    out = {}
    for ev in (c for c in cal.children if c.kind == "VEVENT"):
        m, ovs = out.setdefault(ev.text("UID"), [None, []])
        if ev.get("RECURRENCE-ID"):
            ovs.append(ev)
        else:
            out[ev.text("UID")][0] = ev
    return out


def unescape(v):
    out, it = [], iter(v)
    for c in it:
        if c != "\\":
            out.append(c)
            continue
        n = next(it, "\\")
        out.append("\n" if n in "nN" else n)
    return "".join(out)


def escape(v):
    return v.replace("\\", "\\\\").replace(";", "\\;").replace(",", "\\,").replace("\r\n", "\n").replace("\n", "\\n")


def fold(lines):
    out = []
    for line in lines:
        b, first = line.encode(), True
        while True:
            limit = 75 if first else 74
            if len(b) <= limit:
                out.append((b"" if first else b" ") + b)
                break
            cut = limit
            while cut > 0 and (b[cut] & 0xC0) == 0x80:
                cut -= 1
            out.append((b"" if first else b" ") + b[:cut])
            b, first = b[cut:], False
    return b"\r\n".join(out).decode() + "\r\n"


# times: ("date", date) | ("float", naive) | ("utc", naive) | ("zoned", tz, naive)

def when(p):
    v = p["value"].split(",")[0].strip()
    if p["params"].get("VALUE") == "DATE" or len(v) == 8:
        return ("date", dt.datetime.strptime(v, "%Y%m%d").date())
    if v.endswith("Z"):
        return ("utc", dt.datetime.strptime(v[:-1], "%Y%m%dT%H%M%S"))
    n = dt.datetime.strptime(v, "%Y%m%dT%H%M%S")
    return ("zoned", p["params"]["TZID"], n) if "TZID" in p["params"] else ("float", n)


def when_prop(name, w):
    if w[0] == "date":
        return f"{name};VALUE=DATE:{w[1]:%Y%m%d}"
    if w[0] == "utc":
        return f"{name}:{w[1]:%Y%m%dT%H%M%S}Z"
    if w[0] == "float":
        return f"{name}:{w[1]:%Y%m%dT%H%M%S}"
    return f"{name};TZID={w[1]}:{w[2]:%Y%m%dT%H%M%S}"


def instant(w):
    """The UTC instant of a timed value (floating read as the calendar's zone)."""
    if w[0] == "utc":
        return w[1].replace(tzinfo=UTC)
    tz = ZoneInfo(w[1] if w[0] == "zoned" else CAL_TZ)
    return w[-1].replace(tzinfo=tz).astimezone(UTC)


def key(w):
    """The harness's RECURRENCE-ID key: the date, or the UTC instant."""
    if w[0] == "date":
        return f"D{w[1]:%Y%m%d}"
    return f"U{instant(w):%Y%m%dT%H%M%SZ}"


def zoned_from_utc(t, tz):
    """A UTC instant shown in a zone, as a `when`."""
    if tz in ("UTC", "Etc/UTC"):
        return ("utc", t.astimezone(UTC).replace(tzinfo=None))
    return ("zoned", tz, t.astimezone(ZoneInfo(tz)).replace(tzinfo=None))


def end_of(ev, start):
    p = ev.get("DTEND")
    if p:
        return when(p)
    raise Refused("an event without DTEND (not in the corpus)")


# ------------------------------------------------------------------ HTTP


def http(method, url, token=None, body=None, headers=None, form=None, ok404=False):
    h = {"Accept": "application/json", **(headers or {})}
    data = None
    if token:
        h["Authorization"] = f"Bearer {token}"
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
        h["Content-Type"] = "application/x-www-form-urlencoded"
    elif body is not None:
        data = json.dumps(body).encode()
        h["Content-Type"] = "application/json"
    for attempt in range(6):
        req = urllib.request.Request(url, data=data, method=method, headers=h)
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                raw = r.read()
                return json.loads(raw) if raw else {}
        except urllib.error.HTTPError as e:
            raw = e.read().decode(errors="replace")
            if e.code in (429, 500, 502, 503, 504) or (e.code == 403 and "rateLimitExceeded" in raw):
                time.sleep(float(e.headers.get("Retry-After") or 2 ** attempt))
                continue
            if e.code == 404 and ok404:
                return None
            where = re.sub(r"/[A-Za-z0-9_=-]{40,}", "/<id>", url.split("?")[0])
            raise RuntimeError(f"{method} {where}: HTTP {e.code}: {raw[:400]}")
    raise RuntimeError(f"{method} {url.split('?')[0]}: still throttled")


def secret(name):
    with open(os.path.join(SECRETS, name)) as f:
        return json.load(f)


def harness(*args):
    return subprocess.run([HARNESS, *args], check=True, capture_output=True, text=True).stdout


class Redact:
    """Replaces the owner's addresses and names with placeholders."""

    def __init__(self):
        self.pairs = []

    def add(self, real, placeholder):
        if real:
            self.pairs.append((real, placeholder))
            self.pairs.sort(key=lambda p: -len(p[0]))

    def __call__(self, s):
        for real, ph in self.pairs:
            s = re.sub(re.escape(real), ph, s, flags=re.I)
        # any other Google calendar address
        return re.sub(r"[\w.%-]+(@|%40)group\.calendar\.google\.com", "other-calendar@example.invalid", s)


# ------------------------------------------------------------------ the run


def write_ics(cal, groups, tzids):
    """iCalendar text from {uid: [VEVENT lines...]}, with the source's VTIMEZONEs used."""
    lines = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//nitya//EXP-20261123-bhfzfs providers.py//EN"]
    for tz in (c for c in cal.children if c.kind == "VTIMEZONE"):
        if tz.text("TZID") in tzids:
            lines += tz.lines
    for evs in groups:
        lines += evs
    lines.append("END:VCALENDAR")
    return fold(lines)


def tzids_of(groups):
    return {m.group(1) for evs in groups for l in evs for m in [re.search(r";TZID=([^;:]+)", l)] if m}


def run(driver, corpus, out):
    for d in ("expected", "got", "edited-sent", "edited"):
        os.makedirs(os.path.join(out, d), exist_ok=True)
    cases = sorted(f[:-4] for f in os.listdir(corpus) if f.endswith(".ics"))
    only = os.environ.get("CASES")
    if only:
        cases = [c for c in cases if any(c.startswith(o) for o in only.split(","))]
    rows, detail = ["case\tleg\tmeaning\tinstances_missing\tinstances_extra\tprops_lost\tprops_added"], []
    counts = {"cases": len(cases), "import_same": 0, "writeback_same": 0, "refused": 0, "uid_kept": 0}
    red = driver.red

    def put(sub, name, text):
        path = os.path.join(out, sub, f"{name}.ics")
        with open(path, "w") as f:
            f.write(fold([red(l) for l in unfold(text)]))  # before folding: an address may span a fold
        return path

    def cmp(name, leg, a, b):
        r = harness("compare", name, leg, a, b).split("\n", 1)
        rows.append(r[0])
        if r[1].strip():
            detail.append(r[1])
        return r[0].split("\t")[2] == "same"

    driver.setup()
    try:
        for name in cases:
            src = open(os.path.join(corpus, f"{name}.ics")).read()
            src = driver.adapt(name, src)
            exp_path = put("expected", name, src)
            cal = vcal(parse(src))
            try:
                ids = driver.create(name, cal)
            except Refused as e:
                counts["refused"] += 1
                rows.append(f"{name}\timport\trefused\t-\t-\t-\t-")
                detail.append(f"## {name} / import\nrefused before writing: {e}\n\n")
                continue
            except RuntimeError as e:
                rows.append(f"{name}\timport\terror\t-\t-\t-\t-")
                detail.append(f"## {name} / import\nprovider error: {red(str(e))}\n\n")
                continue
            got, kept = driver.read(name, cal, ids)
            counts["uid_kept"] += kept
            got_path = put("got", name, got)
            if cmp(name, "import", exp_path, got_path):
                counts["import_same"] += 1
            # nitya edits what it read and writes it back
            edited = harness("edit", name, got_path)
            sent_path = put("edited-sent", name, edited)
            try:
                driver.update(name, vcal(parse(edited)), ids)
            except (Refused, RuntimeError) as e:
                rows.append(f"{name}\twrite-back\terror\t-\t-\t-\t-")
                detail.append(f"## {name} / write-back\n{red(str(e))}\n\n")
                continue
            got2, _ = driver.read(name, cal, ids)
            got2_path = put("edited", name, got2)
            same = cmp(name, "write-back", sent_path, got2_path)
            changed = harness("compare", name, "x", got_path, got2_path).split("\t")[2] != "same"
            if same and changed:
                counts["writeback_same"] += 1
            if not changed:
                rows[-1] = rows[-1].replace("\twrite-back\t", "\twrite-back(no-op!)\t", 1)
        driver.after(counts, rows, detail)
    finally:
        driver.teardown()
    with open(os.path.join(out, "table.tsv"), "w") as f:
        f.write("\n".join(rows) + "\n")
    with open(os.path.join(out, "details.md"), "w") as f:
        f.write("".join(detail))
    print("\n".join(rows))
    print("===RESULT=== " + json.dumps(counts))


# ------------------------------------------------------------------ Google

G = "https://www.googleapis.com/calendar/v3"
G_STATUS = {"CONFIRMED": "confirmed", "TENTATIVE": "tentative", "CANCELLED": "cancelled"}
G_PART = {"NEEDS-ACTION": "needsAction", "ACCEPTED": "accepted", "DECLINED": "declined", "TENTATIVE": "tentative"}


def g_time(w):
    if w[0] == "date":
        return {"date": w[1].isoformat()}
    if w[0] == "utc":
        return {"dateTime": w[1].isoformat() + "Z", "timeZone": "UTC"}
    if w[0] == "float":  # Google has no floating time: the calendar's zone
        return {"dateTime": w[1].isoformat(), "timeZone": CAL_TZ}
    return {"dateTime": w[2].isoformat(), "timeZone": w[1]}


def g_when(t):
    if "date" in t:
        return ("date", dt.date.fromisoformat(t["date"]))
    inst = dt.datetime.fromisoformat(t["dateTime"].replace("Z", "+00:00"))
    return zoned_from_utc(inst, t.get("timeZone") or "UTC")


def minutes_trigger(m):
    if m and m % 1440 == 0:
        return f"-P{m // 1440}D"
    if m and m % 60 == 0:
        return f"-PT{m // 60}H"
    return f"-PT{m}M"


def trigger_minutes(t):
    m = re.fullmatch(r"-P(?:(\d+)W)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)?", t)
    if not m:
        raise Refused(f"alarm trigger {t} is not a time before the start")
    w, d, h, mi, s = (int(x or 0) for x in m.groups())
    return w * 10080 + d * 1440 + h * 60 + mi + s // 60


class Google:
    def __init__(self, red):
        self.red = red
        c = secret("google_client.json")["installed"]
        t = secret("google_personal_token.json")
        tok = http("POST", t["token_uri"], form={"grant_type": "refresh_token", "refresh_token": t["refresh_token"],
                                                 "client_id": c["client_id"], "client_secret": c["client_secret"]})
        self.tok = tok["access_token"]
        me = http("GET", f"{G}/calendars/primary", self.tok)
        red.add(me["id"], "owner-google@example.invalid")
        red.add(me.get("summary"), "Owner")

    def setup(self):
        self.cal = http("POST", f"{G}/calendars", self.tok, {"summary": CAL_NAME, "timeZone": CAL_TZ})["id"]
        self.red.add(self.cal, "nitya-spike-calendar@example.invalid")

    def teardown(self):
        http("DELETE", f"{G}/calendars/{urllib.parse.quote(self.cal)}", self.tok, ok404=True)

    def adapt(self, name, src):
        # Google imports an event only when the calendar's owner organizes it or attends
        # (participantIsNeitherOrganizerNorAttendee), and a secondary calendar's owner is the
        # calendar itself, not the account (the account's address was refused too). The
        # corpus's owner@example.org stands for that owner, so it becomes the calendar's id.
        # Import sends no mail.
        return src.replace("owner@example.org", self.cal) if name.startswith("18-") else src

    def after(self, counts, rows, detail):
        pass

    def body(self, ev, master, importing=True):
        b = {"summary": ev.text("SUMMARY"), "start": g_time(when(ev.get("DTSTART"))),
             "end": g_time(end_of(ev, None))}
        if ev.get("DESCRIPTION"):
            b["description"] = ev.text("DESCRIPTION")
        if ev.get("LOCATION"):
            b["location"] = ev.text("LOCATION")
        b["status"] = G_STATUS[ev.text("STATUS") or "CONFIRMED"]
        b["transparency"] = "transparent" if ev.text("TRANSP") == "TRANSPARENT" else "opaque"
        cls = ev.text("CLASS")
        if cls:
            b["visibility"] = {"PRIVATE": "private", "CONFIDENTIAL": "confidential", "PUBLIC": "public"}[cls]
        if ev.get("ORGANIZER") and importing:  # writable only on import
            o = ev.get("ORGANIZER")
            b["organizer"] = {"email": o["value"][7:], "displayName": o["params"].get("CN", "")}
        if ev.all("ATTENDEE"):
            b["attendees"] = [{"email": a["value"][7:], "displayName": a["params"].get("CN", ""),
                               "responseStatus": G_PART[a["params"].get("PARTSTAT", "NEEDS-ACTION")],
                               "optional": a["params"].get("ROLE") == "OPT-PARTICIPANT",
                               **({"resource": True} if a["params"].get("CUTYPE") == "ROOM" else {})}
                              for a in ev.all("ATTENDEE")]
        alarms = [c for c in ev.children if c.kind == "VALARM"]
        b["reminders"] = {"useDefault": False, "overrides": [
            {"method": "email" if a.text("ACTION") == "EMAIL" else "popup",
             "minutes": trigger_minutes(a.get("TRIGGER")["value"])} for a in alarms]}
        if ev.get("URL"):
            b["source"] = {"url": ev.get("URL")["value"], "title": "URL"}
        if ev.all("ATTACH"):
            b["attachments"] = [{"fileUrl": a["value"], "mimeType": a["params"].get("FMTTYPE", ""),
                                 "title": a["value"].rsplit("/", 1)[-1]} for a in ev.all("ATTACH")]
        # what Google has no field for, kept as private extended properties (a nitya choice)
        extra = {p["name"]: p["line"] for p in ev.props if p["name"].startswith("X-") or p["name"] in ("CATEGORIES", "PRIORITY")}
        if extra:
            b["extendedProperties"] = {"private": extra}
        if ev.get("SEQUENCE"):
            b["sequence"] = int(ev.get("SEQUENCE")["value"])
        if master and (ev.get("RRULE") or ev.get("RDATE") or ev.get("EXDATE")):
            b["recurrence"] = [p["line"] for p in ev.props if p["name"] in ("RRULE", "RDATE", "EXDATE")]
        return b

    def create(self, name, cal):
        ids = {}
        for uid, (m, ovs) in by_uid(cal).items():
            for o in ovs:
                if o.get("RECURRENCE-ID")["params"].get("RANGE"):
                    raise Refused("RECURRENCE-ID;RANGE=THISANDFUTURE: Google has no range; a driver must split the series (case 10b)")
            q = f"{G}/calendars/{urllib.parse.quote(self.cal)}/events/import?supportsAttachments=true"
            mb = self.body(m, True)
            mb["iCalUID"] = uid
            ids[uid] = http("POST", q, self.tok, mb)["id"]
            for o in ovs:
                ob = self.body(o, False)
                ob["iCalUID"] = uid
                ob["recurringEventId"] = ids[uid]
                ob["originalStartTime"] = g_time(when(o.get("RECURRENCE-ID")))
                http("POST", q, self.tok, ob)
        return ids

    def items(self, uid):
        q = urllib.parse.urlencode({"iCalUID": uid, "showDeleted": "true", "maxResults": 2500})
        return http("GET", f"{G}/calendars/{urllib.parse.quote(self.cal)}/events?{q}", self.tok).get("items", [])

    def vevent(self, uid, e, master):
        L = ["BEGIN:VEVENT", f"UID:{uid}", f"DTSTAMP:{dt.datetime.now(UTC):%Y%m%dT%H%M%SZ}"]
        if not master:
            L.append(when_prop("RECURRENCE-ID", g_when(e["originalStartTime"])))
        if "start" in e:
            L += [when_prop("DTSTART", g_when(e["start"])), when_prop("DTEND", g_when(e["end"]))]
        if master:
            L += e.get("recurrence", [])
        if e.get("summary") is not None:
            L.append("SUMMARY:" + escape(e["summary"]))
        if e.get("description"):
            L.append("DESCRIPTION:" + escape(e["description"]))
        if e.get("location"):
            L.append("LOCATION:" + escape(e["location"]))
        st = {v: k for k, v in G_STATUS.items()}[e.get("status", "confirmed")]
        if st != "CONFIRMED":
            L.append(f"STATUS:{st}")
        if e.get("transparency") == "transparent":
            L.append("TRANSP:TRANSPARENT")
        if e.get("visibility") in ("private", "confidential", "public"):
            L.append("CLASS:" + e["visibility"].upper())
        o = e.get("organizer")
        if o and e.get("attendees"):  # Google names an organizer for every event; iCalendar only for meetings
            L.append(f"ORGANIZER;CN={o.get('displayName', '')}:mailto:{o['email']}")
        part = {v: k for k, v in G_PART.items()}
        for a in e.get("attendees", []):
            role = "OPT-PARTICIPANT" if a.get("optional") else "REQ-PARTICIPANT"
            cut = ";CUTYPE=ROOM" if a.get("resource") else ""
            L.append(f"ATTENDEE;CN={a.get('displayName', '')}{cut};ROLE={role};PARTSTAT={part[a.get('responseStatus', 'needsAction')]}:mailto:{a['email']}")
        if e.get("source"):
            L.append("URL:" + e["source"]["url"])
        for a in e.get("attachments", []):
            L.append(f"ATTACH;FMTTYPE={a.get('mimeType', '')}:{a['fileUrl']}")
        L += e.get("extendedProperties", {}).get("private", {}).values()
        if e.get("sequence"):
            L.append(f"SEQUENCE:{e['sequence']}")
        for r in (e.get("reminders") or {}).get("overrides", []):
            L += ["BEGIN:VALARM", "ACTION:" + ("EMAIL" if r["method"] == "email" else "DISPLAY"),
                  "DESCRIPTION:Reminder", "TRIGGER:" + minutes_trigger(r["minutes"]), "END:VALARM"]
        L.append("END:VEVENT")
        return L

    def read(self, name, cal, ids):
        groups, kept = [], 1
        for uid in by_uid(cal):
            its = self.items(uid)
            kept &= all(i.get("iCalUID") == uid for i in its)
            master = [i for i in its if "originalStartTime" not in i]
            ovs = [i for i in its if "originalStartTime" in i]
            evs = []
            for m in master:
                evs += self.vevent(uid, m, True)
            for o in ovs:
                if o.get("status") == "cancelled" and "start" not in o:
                    # a cancelled instance comes back as a bare stub: an EXDATE on the master
                    ex = when_prop("EXDATE", g_when(o["originalStartTime"]))
                    i = evs.index("END:VEVENT")
                    evs.insert(i, ex)
                else:
                    evs += self.vevent(uid, o, False)
            groups.append(evs)
        return write_ics(cal, groups, tzids_of(groups)), int(kept)

    def update(self, name, edited, ids):
        base = f"{G}/calendars/{urllib.parse.quote(self.cal)}/events"
        for uid, (m, ovs) in by_uid(edited).items():
            mid = ids[uid]
            mb = self.body(self.real(m), True, importing=False)
            http("PUT", f"{base}/{mid}?sendUpdates=none&supportsAttachments=true", self.tok, mb)
            existing = {key(g_when(i["originalStartTime"])): i["id"] for i in self.items(uid) if "originalStartTime" in i}
            for o in ovs:
                k = key(when(o.get("RECURRENCE-ID")))
                oid = existing.get(k)
                if not oid:
                    q = urllib.parse.urlencode({"timeMin": WINDOW[0], "timeMax": WINDOW[1], "showDeleted": "true", "maxResults": 2500})
                    inst = http("GET", f"{base}/{mid}/instances?{q}", self.tok).get("items", [])
                    oid = next((i["id"] for i in inst if key(g_when(i["originalStartTime"])) == k), None)
                    if not oid:
                        raise Refused(f"no instance at {k} to override")
                ob = self.body(self.real(o), False, importing=False)
                http("PUT", f"{base}/{oid}?sendUpdates=none&supportsAttachments=true", self.tok, ob)

    def real(self, ev):
        """Placeholders back to the owner's real values before writing."""
        for p in ev.props:
            for real, ph in self.red.pairs:
                p["value"] = p["value"].replace(ph, real)
                p["line"] = p["line"].replace(ph, real)
                p["params"] = {k: v.replace(ph, real) for k, v in p["params"].items()}
        return ev


# ------------------------------------------------------------------ Graph

MG = "https://graph.microsoft.com/v1.0"
DAYS = {"MO": "monday", "TU": "tuesday", "WE": "wednesday", "TH": "thursday", "FR": "friday", "SA": "saturday", "SU": "sunday"}
INDEX = {1: "first", 2: "second", 3: "third", 4: "fourth", -1: "last"}
# The Windows zone names Graph may answer with, for the zones in the corpus.
WINDOWS = {"GMT Standard Time": "Europe/London", "Eastern Standard Time": "America/New_York",
           "India Standard Time": "Asia/Kolkata", "UTC": "UTC", "tzone://Microsoft/Utc": "UTC",
           "Greenwich Standard Time": "Europe/London"}
M_PART = {"none": "NEEDS-ACTION", "notResponded": "NEEDS-ACTION", "accepted": "ACCEPTED", "declined": "DECLINED",
          "tentativelyAccepted": "TENTATIVE", "organizer": "ACCEPTED"}


def utc_of(s):
    """Graph's DateTimeOffset or UTC dateTime ("2027-03-03T08:00:00.0000000Z") as an aware UTC datetime."""
    return dt.datetime.fromisoformat(s[:19]).replace(tzinfo=UTC)


def iana(tz):
    return WINDOWS.get(tz, tz)


def m_time(w):
    if w[0] == "date":
        return {"dateTime": f"{w[1].isoformat()}T00:00:00", "timeZone": CAL_TZ}
    if w[0] == "utc":
        return {"dateTime": w[1].isoformat(), "timeZone": "UTC"}
    if w[0] == "float":  # no floating time in Graph either
        return {"dateTime": w[1].isoformat(), "timeZone": CAL_TZ}
    return {"dateTime": w[2].isoformat(), "timeZone": w[1]}


def rrule_parts(v):
    return dict(x.split("=", 1) for x in v.split(";"))


def m_recurrence(m):
    """An RRULE as Graph's patternedRecurrence, or Refused."""
    if m.get("RDATE"):
        raise Refused("RDATE: Graph's recurrence has no extra dates")
    r = rrule_parts(m.get("RRULE")["value"])
    start = when(m.get("DTSTART"))
    local = start[1] if start[0] == "date" else start[-1]
    d0 = local if isinstance(local, dt.date) and not isinstance(local, dt.datetime) else local.date()
    tz = start[1] if start[0] == "zoned" else ("UTC" if start[0] == "utc" else CAL_TZ)
    unknown = set(r) - {"FREQ", "INTERVAL", "COUNT", "UNTIL", "BYDAY", "BYMONTHDAY", "BYMONTH", "BYSETPOS", "WKST"}
    if unknown:
        raise Refused(f"RRULE parts Graph cannot express: {sorted(unknown)}")
    p = {"interval": int(r.get("INTERVAL", 1))}
    byday = r.get("BYDAY", "")
    days = [re.fullmatch(r"([+-]?\d*)(\w\w)", d).groups() for d in byday.split(",") if d]
    freq = r["FREQ"]
    if freq == "DAILY" and not byday:
        p["type"] = "daily"
    elif freq == "WEEKLY":
        if any(n for n, _ in days):
            raise Refused("WEEKLY with ordinal BYDAY")
        p["type"] = "weekly"
        p["daysOfWeek"] = [DAYS[d] for _, d in days] or [d0.strftime("%A").lower()]
        p["firstDayOfWeek"] = DAYS[r.get("WKST", "MO")]
    elif freq == "MONTHLY" and not byday:
        p["type"] = "absoluteMonthly"
        p["dayOfMonth"] = int(r.get("BYMONTHDAY", d0.day))
    elif freq == "MONTHLY":
        pos = r.get("BYSETPOS")
        if pos and all(not n for n, _ in days) and int(pos) in INDEX:
            # Graph's relative pattern over several days is "the n-th of any of these days"
            p.update(type="relativeMonthly", index=INDEX[int(pos)], daysOfWeek=[DAYS[d] for _, d in days])
        elif not pos and len(days) == 1 and days[0][0] and int(days[0][0]) in INDEX:
            p.update(type="relativeMonthly", index=INDEX[int(days[0][0])], daysOfWeek=[DAYS[days[0][1]]])
        else:
            raise Refused(f"MONTHLY rule Graph cannot express: {m.get('RRULE')['value']}")
    elif freq == "YEARLY" and not byday:
        p.update(type="absoluteYearly", month=int(r.get("BYMONTH", d0.month)), dayOfMonth=int(r.get("BYMONTHDAY", d0.day)))
    else:
        raise Refused(f"RRULE Graph cannot express: {m.get('RRULE')['value']}")
    rng = {"startDate": d0.isoformat(), "recurrenceTimeZone": tz}
    if "COUNT" in r:
        rng.update(type="numbered", numberOfOccurrences=int(r["COUNT"]))
    elif "UNTIL" in r:
        u = r["UNTIL"]
        last = (dt.datetime.strptime(u[:15], "%Y%m%dT%H%M%S").replace(tzinfo=UTC).astimezone(ZoneInfo(tz if tz != "UTC" else "UTC")).date()
                if "T" in u else dt.datetime.strptime(u, "%Y%m%d").date())
        rng.update(type="endDate", endDate=last.isoformat())
    else:
        rng["type"] = "noEnd"
    return {"pattern": p, "range": rng}


def m_rrule(rec, start):
    """Graph's patternedRecurrence back to an RRULE line."""
    p, r = rec["pattern"], rec["range"]
    inv = {v: k for k, v in DAYS.items()}
    idx = {v: k for k, v in INDEX.items()}
    parts = {"daily": ["FREQ=DAILY"], "weekly": ["FREQ=WEEKLY"], "absoluteMonthly": ["FREQ=MONTHLY"],
             "relativeMonthly": ["FREQ=MONTHLY"], "absoluteYearly": ["FREQ=YEARLY"], "relativeYearly": ["FREQ=YEARLY"]}[p["type"]]
    if p.get("interval", 1) != 1:
        parts.append(f"INTERVAL={p['interval']}")
    if p["type"] == "weekly":
        parts.append("BYDAY=" + ",".join(inv[d] for d in p["daysOfWeek"]))
    elif p["type"] == "absoluteMonthly":
        parts.append(f"BYMONTHDAY={p['dayOfMonth']}")
    elif p["type"] == "relativeMonthly":
        if len(p["daysOfWeek"]) == 1:
            parts.append(f"BYDAY={idx[p['index']]}{inv[p['daysOfWeek'][0]]}")
        else:
            parts += ["BYDAY=" + ",".join(inv[d] for d in p["daysOfWeek"]), f"BYSETPOS={idx[p['index']]}"]
    elif p["type"] == "absoluteYearly":
        parts += [f"BYMONTH={p['month']}", f"BYMONTHDAY={p['dayOfMonth']}"]
    if r["type"] == "numbered":
        parts.append(f"COUNT={r['numberOfOccurrences']}")
    elif r["type"] == "endDate":
        end = dt.date.fromisoformat(r["endDate"])
        if start[0] == "date":
            parts.append(f"UNTIL={end:%Y%m%d}")
        else:
            tz = iana(r.get("recurrenceTimeZone") or CAL_TZ)
            u = dt.datetime.combine(end, dt.time(23, 59, 59)).replace(tzinfo=ZoneInfo(tz)).astimezone(UTC)
            parts.append(f"UNTIL={u:%Y%m%dT%H%M%S}Z")
    return "RRULE:" + ";".join(parts)


class Graph:
    def __init__(self, red):
        self.red = red
        t = secret("msft_personal_token.json")
        tok = http("POST", t["token_uri"], form={"grant_type": "refresh_token", "refresh_token": t["refresh_token"],
                                                 "client_id": t["client_id"], "scope": "https://graph.microsoft.com/.default offline_access"})
        self.tok = tok["access_token"]
        me = self.get("/me?$select=displayName,mail,userPrincipalName")
        self.me = me.get("mail") or me["userPrincipalName"]
        red.add(self.me, "owner-microsoft@example.invalid")
        red.add(me.get("userPrincipalName"), "owner-microsoft@example.invalid")
        red.add(me.get("displayName"), "Owner")
        # the attendee for case 18: the owner's Google account
        self.google = Google(red)

    H = {"Prefer": 'outlook.timezone="UTC", outlook.body-content-type="text"'}

    def get(self, path):
        return http("GET", MG + path, self.tok, headers=self.H)

    def setup(self):
        self.cal = http("POST", f"{MG}/me/calendars", self.tok, {"name": CAL_NAME})["id"]
        self.created = []

    def teardown(self):
        # deleting each event first, so the meeting of case 18 is cancelled for its attendee
        for mid in self.created:
            http("DELETE", f"{MG}/me/events/{mid}", self.tok, ok404=True)
        http("DELETE", f"{MG}/me/calendars/{self.cal}", self.tok, ok404=True)

    def adapt(self, name, src):
        if not name.startswith("18-"):
            return src
        g = next(r for r, ph in self.red.pairs if ph == "owner-google@example.invalid")
        lines = [l for l in unfold(src) if not l.startswith(("ATTENDEE", "ORGANIZER"))]
        i = lines.index("END:VEVENT")
        lines[i:i] = [f"ORGANIZER;CN=Owner:mailto:{self.me}",
                      f"ATTENDEE;CN=Owner (Google);ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{g}"]
        return fold(lines)

    def body(self, ev, master):
        start = when(ev.get("DTSTART"))
        b = {"subject": ev.text("SUMMARY"), "start": m_time(start), "end": m_time(end_of(ev, start)),
             "isAllDay": start[0] == "date",
             "body": {"contentType": "text", "content": ev.text("DESCRIPTION")},
             "location": {"displayName": ev.text("LOCATION")},
             "showAs": "free" if ev.text("TRANSP") == "TRANSPARENT" else ("tentative" if ev.text("STATUS") == "TENTATIVE" else "busy"),
             "sensitivity": {"PRIVATE": "private", "CONFIDENTIAL": "confidential"}.get(ev.text("CLASS"), "normal"),
             "categories": [c for p in ev.all("CATEGORIES") for c in p["value"].split(",")]}
        alarms = [c for c in ev.children if c.kind == "VALARM"]
        b["isReminderOn"] = bool(alarms)
        if alarms:
            b["reminderMinutesBeforeStart"] = trigger_minutes(alarms[0].get("TRIGGER")["value"])
        if ev.all("ATTENDEE"):
            b["attendees"] = [{"emailAddress": {"address": a["value"][7:], "name": a["params"].get("CN", "")},
                               "type": "resource" if a["params"].get("CUTYPE") == "ROOM" else
                               ("optional" if a["params"].get("ROLE") == "OPT-PARTICIPANT" else "required")}
                              for a in ev.all("ATTENDEE")]
        if master and ev.get("RRULE"):
            b["recurrence"] = m_recurrence(ev)
        elif master and ev.get("RDATE"):
            raise Refused("RDATE without RRULE")
        return b

    def instances(self, mid):
        out, url = [], f"/me/events/{mid}/instances?startDateTime={WINDOW[0]}&endDateTime={WINDOW[1]}&$top=200&$select=id,type,originalStart,start,end,isAllDay"
        while url:
            r = self.get(url)
            out += r["value"]
            url = r.get("@odata.nextLink", "").replace(MG, "")
        return out

    def inst_key(self, i, allday):
        if allday:
            return "D" + i["start"]["dateTime"][:10].replace("-", "") if i["type"] == "occurrence" else None
        return f"U{utc_of(i['originalStart']):%Y%m%dT%H%M%SZ}"

    def rec_date(self, mid, original_start):
        """An all-day series' instance date from an originalStart (an instant: midnight
        in the series' recurrence zone, it seems; the run shows whether that holds)."""
        tz = iana((self.get(f"/me/events/{mid}?$select=recurrence").get("recurrence") or {}).get("range", {}).get("recurrenceTimeZone") or CAL_TZ)
        return utc_of(original_start).astimezone(ZoneInfo(tz)).strftime("%Y%m%d")

    def exceptions(self, uid, cal, m):
        """Overrides and EXDATEs of the source, applied to the series by instance."""
        mid = self.ids[uid]
        allday = when(m.get("DTSTART"))[0] == "date"
        insts = self.instances(mid)
        by_key = {}
        for i in insts:
            if allday and i["type"] == "exception":
                # an exception's own start may be timed; its originalStart is the date
                by_key["D" + self.rec_date(mid, i["originalStart"])] = i
            else:
                by_key[self.inst_key(i, allday)] = i
        return by_key

    def create(self, name, cal):
        self.ids = getattr(self, "ids", {})
        groups = by_uid(cal)
        for uid, (m, ovs) in groups.items():
            for o in ovs:
                if o.get("RECURRENCE-ID")["params"].get("RANGE"):
                    raise Refused("RECURRENCE-ID;RANGE=THISANDFUTURE: Graph has no range; a driver must split the series (case 10b)")
            self.body(m, True)  # refuse before any write
        ids = {}
        for uid, (m, ovs) in groups.items():
            b = self.body(m, True)
            b["transactionId"] = f"{uid}-{int(time.time())}"
            ev = http("POST", f"{MG}/me/calendars/{self.cal}/events", self.tok, b, headers=self.H)
            ids[uid] = self.ids[uid] = ev["id"]
            self.created.append(ev["id"])
            self.icaluid = getattr(self, "icaluid", {})
            self.icaluid[uid] = ev.get("iCalUId")
            self.apply_exceptions(uid, m, ovs)
        return ids

    def apply_exceptions(self, uid, m, ovs):
        if not m.get("RRULE"):
            return
        by_key = self.exceptions(uid, None, m)
        gone = {key(when({**p, "value": v})) for p in m.all("EXDATE") for v in p["value"].split(",")}
        for k in sorted(gone & set(by_key)):
            http("DELETE", f"{MG}/me/events/{by_key[k]['id']}", self.tok, ok404=True)
        for o in ovs:
            k = key(when(o.get("RECURRENCE-ID")))
            if k not in by_key:
                raise Refused(f"no instance at {k} to override")
            if o.text("STATUS") == "CANCELLED":
                http("DELETE", f"{MG}/me/events/{by_key[k]['id']}", self.tok)
                continue
            b = self.body(o, False)
            b.pop("recurrence", None)
            http("PATCH", f"{MG}/me/events/{by_key[k]['id']}", self.tok, b, headers=self.H)

    def vevent(self, uid, e, master, rid=None):
        allday = e.get("isAllDay")
        tz = iana(e.get("originalStartTimeZone") or "UTC")

        def w(t):
            if allday:
                return ("date", dt.date.fromisoformat(t["dateTime"][:10]))
            inst = dt.datetime.fromisoformat(t["dateTime"][:19]).replace(tzinfo=UTC)
            try:
                return zoned_from_utc(inst, tz)
            except Exception:
                return zoned_from_utc(inst, "UTC")

        L = ["BEGIN:VEVENT", f"UID:{uid}", f"DTSTAMP:{dt.datetime.now(UTC):%Y%m%dT%H%M%SZ}"]
        if rid:
            L.append(rid)
        start = w(e["start"])
        L += [when_prop("DTSTART", start), when_prop("DTEND", w(e["end"]))]
        if master and e.get("recurrence"):
            L.append(m_rrule(e["recurrence"], start))
        L.append("SUMMARY:" + escape(e.get("subject") or ""))
        body = (e.get("body") or {}).get("content") or ""
        if body.strip():
            L.append("DESCRIPTION:" + escape(body))
        loc = (e.get("location") or {}).get("displayName")
        if loc:
            L.append("LOCATION:" + escape(loc))
        if e.get("showAs") == "free":
            L.append("TRANSP:TRANSPARENT")
        if e.get("showAs") == "tentative":
            L.append("STATUS:TENTATIVE")
        if e.get("isCancelled"):
            L.append("STATUS:CANCELLED")
        if e.get("sensitivity") in ("private", "confidential"):
            L.append("CLASS:" + e["sensitivity"].upper())
        if e.get("categories"):
            L.append("CATEGORIES:" + ",".join(e["categories"]))
        if e.get("attendees"):
            o = e["organizer"]["emailAddress"]
            L.append(f"ORGANIZER;CN={o.get('name', '')}:mailto:{o['address']}")
        for a in e.get("attendees", []):
            role = {"optional": "OPT-PARTICIPANT", "resource": "NON-PARTICIPANT"}.get(a["type"], "REQ-PARTICIPANT")
            ps = M_PART[a.get("status", {}).get("response", "none")]
            L.append(f"ATTENDEE;CN={a['emailAddress'].get('name', '')};ROLE={role};PARTSTAT={ps}:mailto:{a['emailAddress']['address']}")
        if e.get("isReminderOn"):
            L += ["BEGIN:VALARM", "ACTION:DISPLAY", "DESCRIPTION:Reminder",
                  "TRIGGER:" + minutes_trigger(e["reminderMinutesBeforeStart"]), "END:VALARM"]
        L.append("END:VEVENT")
        return L

    SEL = "$select=id,iCalUId,subject,body,start,end,isAllDay,originalStartTimeZone,recurrence,location,showAs,sensitivity,categories,isCancelled,organizer,attendees,isReminderOn,reminderMinutesBeforeStart,type,originalStart"

    def read(self, name, cal, ids):
        groups, kept = [], 1
        for uid, (m, _) in by_uid(cal).items():
            e = self.get(f"/me/events/{ids[uid]}?{self.SEL}")
            kept &= e.get("iCalUId") == uid
            evs = self.vevent(uid, e, True)
            if e.get("recurrence"):
                # EXDATEs: what the series' own rule generates and has no instance now
                full = write_ics(cal, [evs], tzids_of([evs]))
                gen = set(re.findall(r"\| rid (\S+) \|", subprocess.run([HARNESS, "show", "/dev/stdin"], input=full, capture_output=True, text=True, check=True).stdout))
                insts = self.instances(ids[uid])
                start = when(content_line(next(l for l in evs if l.startswith("DTSTART"))))
                allday = start[0] == "date"
                present, exc = set(), []
                for i in insts:
                    if i["type"] == "exception":
                        exc.append(i)
                        t = utc_of(i["originalStart"])
                        present.add(("D" + self.rec_date(ids[uid], i["originalStart"])) if allday else f"U{t:%Y%m%dT%H%M%SZ}")
                    else:
                        present.add(self.inst_key(i, allday))
                gone = sorted(gen - present)
                ex_lines = []
                for k in gone:
                    if allday:
                        ex_lines.append(f"EXDATE;VALUE=DATE:{k[1:]}")
                    else:
                        t = dt.datetime.strptime(k[1:], "%Y%m%dT%H%M%SZ").replace(tzinfo=UTC)
                        ex_lines.append(when_prop("EXDATE", zoned_from_utc(t, start[1] if start[0] == "zoned" else "UTC")))
                i = evs.index("END:VEVENT")
                evs[i:i] = ex_lines
                for x in exc:
                    full_x = self.get(f"/me/events/{x['id']}?{self.SEL}")
                    t = utc_of(x["originalStart"])
                    if allday:
                        rid = "RECURRENCE-ID;VALUE=DATE:" + self.rec_date(ids[uid], x["originalStart"])
                    else:
                        rid = when_prop("RECURRENCE-ID", zoned_from_utc(t, start[1] if start[0] == "zoned" else "UTC"))
                    evs += self.vevent(uid, full_x, False, rid)
            groups.append(evs)
        return write_ics(cal, groups, tzids_of(groups)), int(kept)

    def update(self, name, edited, ids):
        for uid, (m, ovs) in by_uid(edited).items():
            m = self.real(m)
            b = self.body(m, True)
            cur = self.get(f"/me/events/{ids[uid]}?$select=recurrence")
            if cur.get("recurrence") and b.get("recurrence"):
                s0 = when(m.get("DTSTART"))
                if m_rrule(cur["recurrence"], s0) == m_rrule(b["recurrence"], s0):
                    # PATCHing an unchanged recurrence would reset every exception
                    b.pop("recurrence")
                    b.pop("start")
                    b.pop("end")
            http("PATCH", f"{MG}/me/events/{ids[uid]}", self.tok, b, headers=self.H)
            self.apply_exceptions(uid, m, [self.real(o) for o in ovs])

    real = Google.real

    def after(self, counts, rows, detail):
        """Prediction 4: answer case 18's invitation from Google; wait for Graph to show it."""
        uid = next((u for u in self.ids if u.startswith("c18")), None)
        if not uid:
            return
        g, guid = self.google, self.icaluid.get(uid)
        found, t0 = None, time.time()
        while time.time() - t0 < 300 and not found:
            q = urllib.parse.urlencode({"iCalUID": guid})
            items = http("GET", f"{G}/calendars/primary/events?{q}", g.tok).get("items", [])
            found = items[0] if items else None
            if not found:
                time.sleep(10)
        res = {"invite_reached_google_s": None, "reply_reached_graph_s": None}
        if found:
            res["invite_reached_google_s"] = round(time.time() - t0)
            me = [a for a in found.get("attendees", []) if a.get("self")]
            for a in me:
                a["responseStatus"] = "accepted"
            http("PATCH", f"{G}/calendars/primary/events/{found['id']}?sendUpdates=all", g.tok, {"attendees": found["attendees"]})
            t1 = time.time()
            while time.time() - t1 < 600:
                e = self.get(f"/me/events/{self.ids[uid]}?$select=attendees")
                if any(a.get("status", {}).get("response") == "accepted" for a in e.get("attendees", [])):
                    res["reply_reached_graph_s"] = round(time.time() - t1)
                    break
                time.sleep(15)
        counts.update(res)
        rows.append(f"18-attendees\titip-reply\t{'same' if res['reply_reached_graph_s'] is not None else 'CHANGED'}\t-\t-\t-\t-")
        detail.append(f"## 18-attendees / itip-reply\n{json.dumps(res)}\n\n")


# ------------------------------------------------------------------ iCloud (CalDAV)

ICLOUD = "https://caldav.icloud.com/"
NS = {"d": "DAV:", "c": "urn:ietf:params:xml:ns:caldav"}


def dav(method, url, auth, body=None, headers=None):
    """One WebDAV request; follows redirects (urllib does not for PROPFIND) and retries
    throttling. Returns (status, headers, text)."""
    h = {"Authorization": auth, **(headers or {})}
    data = body.encode() if isinstance(body, str) else body
    for attempt in range(8):
        req = urllib.request.Request(url, data=data, method=method, headers=h)
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.status, r.headers, r.read().decode()
        except urllib.error.HTTPError as e:
            raw = e.read().decode(errors="replace")
            if e.code in (301, 302, 307, 308) and e.headers.get("Location"):
                url = urllib.parse.urljoin(url, e.headers["Location"])
                continue
            if e.code in (429, 500, 502, 503, 504):
                time.sleep(float(e.headers.get("Retry-After") or 2 ** attempt))
                continue
            where = re.sub(r"/\d{5,}/", "/<dsid>/", urllib.parse.urlsplit(url).path)
            raise RuntimeError(f"{method} {where}: HTTP {e.code}: {raw[:400]}")
    raise RuntimeError(f"{method}: still throttled")


def propfind(url, auth, props):
    import xml.etree.ElementTree as ET
    body = ('<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" '
            'xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop>' + "".join(f"<{x}/>" for x in props) +
            "</d:prop></d:propfind>")
    _, _, text = dav("PROPFIND", url, auth, body, {"Depth": "0", "Content-Type": "application/xml; charset=utf-8"})
    return ET.fromstring(text)


def merge(texts):
    """Several calendar resources as one VCALENDAR: the first one's head, every
    VTIMEZONE once by TZID, every VEVENT."""
    head, tzs, evs = None, {}, []
    for t in texts:
        c = vcal(parse(t))
        if head is None:
            head = [p["line"] for p in c.props]
        for ch in c.children:
            if ch.kind == "VTIMEZONE":
                tzs.setdefault(ch.text("TZID"), ch.lines)
            elif ch.kind == "VEVENT":
                evs.append(ch.lines)
    lines = ["BEGIN:VCALENDAR", *head]
    for t in tzs.values():
        lines += t
    for e in evs:
        lines += e
    return fold(lines + ["END:VCALENDAR"])


class ICloud:
    """iCloud's CalDAV, with an app-specific password. iCloud schedules on the server
    (RFC 6638): saving an event that has attendees sends the invitations itself."""

    def __init__(self, red):
        import base64
        self.red = red
        t = secret("icloud_personal_token.json")
        self.user = t["username"]
        self.auth = "Basic " + base64.b64encode(f"{t['username']}:{t['app_password']}".encode()).decode()
        red.add(self.user, "owner-icloud@example.invalid")
        x = propfind(ICLOUD, self.auth, ["d:current-user-principal"])
        principal = urllib.parse.urljoin(ICLOUD, x.find(".//d:current-user-principal/d:href", NS).text)
        x = propfind(principal, self.auth, ["c:calendar-home-set", "c:calendar-user-address-set", "d:displayname"])
        self.home = urllib.parse.urljoin(principal, x.find(".//c:calendar-home-set/d:href", NS).text)
        addrs = [h.text for h in x.findall(".//c:calendar-user-address-set/d:href", NS)]
        # the account's other scheduling addresses (principal paths, a urn) identify it too
        red.add(urllib.parse.urlsplit(principal).path, "/owner-icloud-principal/")
        for a in addrs:
            if not a.lower().startswith("mailto:"):
                red.add(urllib.parse.urlsplit(a).path if a.startswith(("/", "http")) else a, "owner-icloud-principal")
        mails = [a[7:] for a in addrs if a.lower().startswith("mailto:")]
        for a in mails:
            red.add(a, "owner-icloud@example.invalid")
        name = x.find(".//d:displayname", NS)
        if name is not None and name.text:
            red.add(name.text, "Owner")
        # the organizer for case 18: the Apple ID if iCloud schedules for it, else its first address
        self.me = self.user if self.user.lower() in (m.lower() for m in mails) else (mails[0] if mails else self.user)
        # case 18's attendee: the owner's Microsoft account (the Apple ID is the owner's
        # Gmail address, the only one iCloud organizes as; the owner chose this)
        self.ms = Graph(red)
        self.google = self.ms.google
        self.etags, self.created, self.ids = {}, [], {}

    def setup(self):
        import uuid
        self.cal = self.home.rstrip("/") + f"/{uuid.uuid4().hex.upper()}/"
        body = ('<?xml version="1.0" encoding="utf-8"?><c:mkcalendar xmlns:d="DAV:" '
                'xmlns:c="urn:ietf:params:xml:ns:caldav"><d:set><d:prop>'
                f"<d:displayname>{CAL_NAME}</d:displayname>"
                '<c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set>'
                "</d:prop></d:set></c:mkcalendar>")
        dav("MKCALENDAR", self.cal, self.auth, body, {"Content-Type": "application/xml; charset=utf-8"})

    def teardown(self):
        # each event first, so the meeting of case 18 is cancelled for its attendee
        for url in self.created:
            try:
                dav("DELETE", url, self.auth)
            except RuntimeError:
                pass
        dav("DELETE", self.cal, self.auth)

    def adapt(self, name, src):
        if not name.startswith("18-"):
            return src
        lines = [l for l in unfold(src) if not l.startswith(("ATTENDEE", "ORGANIZER"))]
        i = lines.index("END:VEVENT")
        lines[i:i] = [f"ORGANIZER;CN=Owner:mailto:{self.me}",
                      f"ATTENDEE;CN=Owner (Microsoft);ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{self.ms.me}"]
        return fold(lines)

    def unredact(self, text):
        """Address placeholders back to one real address each (several of the owner's
        iCloud addresses share a placeholder; the organizer's is the one written)."""
        back = {"owner-icloud@example.invalid": self.me,
                "owner-google@example.invalid": next(r for r, ph in self.red.pairs if ph == "owner-google@example.invalid"),
                "owner-microsoft@example.invalid": self.ms.me}
        for ph, real in back.items():
            text = text.replace(ph, real)
        return text

    def resources(self, cal):
        """One resource per UID (RFC 4791 section 4.1), with the calendar's VTIMEZONEs."""
        out = []
        for uid, (m, ovs) in by_uid(cal).items():
            groups = [c.lines for c in ([m] if m else []) + ovs]
            out.append((uid, write_ics(cal, groups, tzids_of(groups))))
        return out

    def create(self, name, cal):
        ids = {}
        for i, (uid, text) in enumerate(self.resources(cal)):
            url = f"{self.cal}{name}-{i}.ics"
            dav("PUT", url, self.auth, text, {"Content-Type": "text/calendar; charset=utf-8", "If-None-Match": "*"})
            self.created.append(url)
            ids[uid] = self.ids[uid] = url
        return ids

    def read(self, name, cal, ids):
        texts, kept = [], 1
        for uid in by_uid(cal):
            _, h, text = dav("GET", ids[uid], self.auth)
            self.etags[ids[uid]] = h.get("ETag")
            kept &= all(e.text("UID") == uid for e in vcal(parse(text)).children if e.kind == "VEVENT")
            texts.append(text)
        return merge(texts), int(kept)

    def update(self, name, edited, ids):
        for uid, text in self.resources(edited):
            url = ids[uid]
            hdr = {"Content-Type": "text/calendar; charset=utf-8"}
            if self.etags.get(url):
                hdr["If-Match"] = self.etags[url]
            dav("PUT", url, self.auth, self.unredact(text), hdr)

    def after(self, counts, rows, detail):
        """Prediction 4: answer case 18's invitation from Outlook.com; wait for iCloud to
        show it, and look for the reply in the organizer's (Gmail) inbox."""
        uid = next((u for u in self.ids if u.startswith("c18")), None)
        if not uid:
            return
        res = {"invite_reached_microsoft_s": None, "reply_reached_icloud_s": None, "reply_in_gmail": None}
        found, t0 = None, time.time()
        q = urllib.parse.quote(f"iCalUId eq '{uid}'")
        while time.time() - t0 < 300 and not found:
            items = self.ms.get(f"/me/events?$filter={q}&$select=id").get("value", [])
            found = items[0] if items else None
            if not found:
                time.sleep(10)
        if found:
            res["invite_reached_microsoft_s"] = round(time.time() - t0)
            http("POST", f"{MG}/me/events/{found['id']}/accept", self.ms.tok, {"sendResponse": True})
            addr = self.ms.me.lower()
            t1 = time.time()
            while time.time() - t1 < 600:
                text = dav("GET", self.ids[uid], self.auth)[2]
                att = [l for l in unfold(text) if l.startswith("ATTENDEE") and addr in l.lower()]
                if any("PARTSTAT=ACCEPTED" in l for l in att):
                    res["reply_reached_icloud_s"] = round(time.time() - t1)
                    break
                time.sleep(15)
            # where the reply went: the organizer's address is the Gmail account
            gq = urllib.parse.urlencode({"q": 'subject:"Accepted: Meeting with every participation status" newer_than:1d'})
            msgs = http("GET", f"https://gmail.googleapis.com/gmail/v1/users/me/messages?{gq}", self.google.tok).get("messages", [])
            res["reply_in_gmail"] = bool(msgs)
        counts.update(res)
        rows.append(f"18-attendees\titip-reply\t{'same' if res['reply_reached_icloud_s'] is not None else 'CHANGED'}\t-\t-\t-\t-")
        detail.append(f"## 18-attendees / itip-reply\n{json.dumps(res)}\n\n")


if __name__ == "__main__":
    which, HARNESS, corpus_dir, out_dir = sys.argv[1:5]
    red = Redact()
    drv = {"google": Google, "graph": Graph, "icloud": ICloud}[which](red)
    run(drv, corpus_dir, out_dir)
