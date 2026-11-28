//! nitya's iCalendar reader and writer (RFC 5545): content lines, unfolded, in nested
//! components, each property kept with its raw line, so a driver can pass lines through
//! that it does not interpret (recurrence rules, X- properties). EXP-20261123-bhfzfs
//! found this enough for every case of its corpus; nitya writes by editing lines of the
//! stored text, never through a full model.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    /// Upper case.
    pub name: String,
    /// Keys upper case, values unquoted.
    pub params: BTreeMap<String, String>,
    /// Still escaped, as on the line.
    pub value: String,
    /// The whole unfolded line.
    pub line: String,
}

impl Prop {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    /// A TEXT value as a person reads it.
    pub fn text(&self) -> String {
        unescape(&self.value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comp {
    /// Upper case; the root of a parse is `ROOT`.
    pub kind: String,
    pub props: Vec<Prop>,
    pub children: Vec<Comp>,
}

impl Comp {
    pub fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }

    /// The unescaped TEXT of `name`, if it is there.
    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name).map(Prop::text)
    }

    /// Every component of `kind` below this one, depth first.
    pub fn find(&self, kind: &str) -> Vec<&Comp> {
        let mut out = Vec::new();
        for c in &self.children {
            if c.kind == kind {
                out.push(c);
            } else {
                out.extend(c.find(kind));
            }
        }
        out
    }
}

/// Lines with their continuations joined; CRLF or LF.
pub fn unfold(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in s.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t'))
            && let Some(last) = out.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// Splits `NAME;P=V;Q="a:b":value`, respecting quoted parameter values.
pub fn content_line(line: &str) -> Prop {
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
    Prop { name, params, value: value.to_string(), line: line.to_string() }
}

/// The components of `s` under a `ROOT`. Lenient: unclosed components are closed at the end.
pub fn parse(s: &str) -> Comp {
    let mut stack = vec![Comp { kind: "ROOT".into(), ..Default::default() }];
    for line in unfold(s) {
        let p = content_line(&line);
        match p.name.as_str() {
            "BEGIN" => stack.push(Comp { kind: p.value.to_ascii_uppercase(), ..Default::default() }),
            "END" if stack.len() > 1 => {
                let c = stack.pop().expect("more than the root");
                stack.last_mut().expect("the root").children.push(c);
            }
            _ => stack.last_mut().expect("the root").props.push(p),
        }
    }
    while stack.len() > 1 {
        let c = stack.pop().expect("more than the root");
        stack.last_mut().expect("the root").children.push(c);
    }
    stack.pop().expect("the root")
}

/// Lines as a calendar file: CRLF, at most 75 octets a line, never inside a character.
pub fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        let mut rest = line.as_str();
        let mut first = true;
        loop {
            let limit = if first { 75 } else { 74 };
            if !first {
                out.push(' ');
            }
            if rest.len() <= limit {
                out.push_str(rest);
                out.push_str("\r\n");
                break;
            }
            let mut cut = limit;
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            out.push_str(&rest[..cut]);
            out.push_str("\r\n");
            rest = &rest[cut..];
            first = false;
        }
    }
    out
}

/// A TEXT value as a person reads it (RFC 5545 section 3.3.11).
pub fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(x) => out.push(x),
            None => out.push('\\'),
        }
    }
    out
}

/// Text as a TEXT value.
pub fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\n").replace('\n', "\\n")
}

/// A parameter value, quoted when it must be.
pub fn param_value(v: &str) -> String {
    if v.contains([':', ';', ',']) { format!("\"{}\"", v.replace('"', "")) } else { v.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_folded_lines_quoted_params_and_nested_components() {
        let s = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:a\\, b\\n c\r\n  continued\r\nATTENDEE;CN=\"Doe; J\";PARTSTAT=ACCEPTED:mailto:j@example.org\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let root = parse(s);
        let ev = root.find("VEVENT");
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].text("SUMMARY").unwrap(), "a, b\n c continued");
        let a = ev[0].get("ATTENDEE").unwrap();
        assert_eq!(a.param("CN"), Some("Doe; J"));
        assert_eq!(a.value, "mailto:j@example.org");
        assert_eq!(ev[0].children[0].kind, "VALARM");
        assert_eq!(ev[0].children[0].get("TRIGGER").unwrap().line, "TRIGGER:-PT15M");
    }

    #[test]
    fn folds_at_75_octets_without_splitting_a_character_and_reads_back() {
        let long = format!("DESCRIPTION:{}", escape(&"é, ünïcode; ".repeat(20)));
        let s = fold(&["BEGIN:VEVENT".into(), long.clone(), "END:VEVENT".into()]);
        assert!(s.split("\r\n").all(|l| l.len() <= 75));
        assert_eq!(unfold(&s)[1], long);
        assert_eq!(parse(&s).find("VEVENT")[0].text("DESCRIPTION").unwrap(), "é, ünïcode; ".repeat(20));
    }

    #[test]
    fn escape_and_unescape_are_inverse() {
        let t = "a;b,c\\d\ne";
        assert_eq!(unescape(&escape(t)), t);
        assert_eq!(param_value("Doe, J"), "\"Doe, J\"");
        assert_eq!(param_value("Doe"), "Doe");
    }
}
