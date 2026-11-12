//! A deterministic KDL v2 printer.
//!
//! kdl-rs's own `Display` and autoformat keep (or guess) source formatting
//! and collapse multiline strings into `\n` escapes (kdl-org/kdl-rs#77). This
//! printer ignores source formatting and comments entirely and always
//! produces the same text for the same document: two-space indentation (by
//! default), entries in order, bare identifiers where KDL allows them, and
//! strings with newlines as `"""` multiline strings.

use core::fmt::Write as _;

use kdl::{KdlDocument, KdlEntry, KdlIdentifier, KdlNode, KdlValue};

use crate::Options;

/// Formats `doc` as KDL v2 text, one node per line with nested children
/// indented.
///
/// ```
/// let doc: kdl::KdlDocument = "a   1 ;b { c \"x\" }".parse().unwrap();
/// assert_eq!(kdl_serde::format(&doc, &Default::default()), "a 1\nb {\n  c \"x\"\n}\n");
/// ```
pub fn format(doc: &KdlDocument, opts: &Options) -> String {
    let mut p = Printer {
        out: String::new(),
        opts,
        compact: false,
    };
    p.nodes(doc.nodes(), 0);
    p.out
}

/// Formats `doc` with each top-level node on one line, children inline:
/// `a 1 { b 2; c 3; }`.
pub fn format_compact(doc: &KdlDocument) -> String {
    let opts = Options::default().multiline_strings(false);
    let mut p = Printer {
        out: String::new(),
        opts: &opts,
        compact: true,
    };
    p.nodes(doc.nodes(), 0);
    p.out
}

struct Printer<'o> {
    out: String,
    opts: &'o Options,
    compact: bool,
}

impl Printer<'_> {
    fn indent(&mut self, depth: usize) {
        for _ in 0..depth * self.opts.indent {
            self.out.push(' ');
        }
    }

    fn nodes(&mut self, nodes: &[KdlNode], depth: usize) {
        for node in nodes {
            if self.compact && depth > 0 {
                self.out.push(' ');
                self.node(node, depth);
                self.out.push(';');
            } else {
                self.indent(depth);
                self.node(node, depth);
                self.out.push('\n');
            }
        }
    }

    fn node(&mut self, node: &KdlNode, depth: usize) {
        if let Some(ty) = node.ty() {
            self.annotation(ty);
        }
        self.ident(node.name().value());
        for entry in node.entries() {
            self.out.push(' ');
            self.entry(entry, depth);
        }
        if let Some(children) = node.children().filter(|c| !c.nodes().is_empty()) {
            if self.compact {
                self.out.push_str(" {");
                self.nodes(children.nodes(), depth + 1);
                self.out.push_str(" }");
            } else {
                self.out.push_str(" {\n");
                self.nodes(children.nodes(), depth + 1);
                self.indent(depth);
                self.out.push('}');
            }
        }
    }

    fn annotation(&mut self, ty: &KdlIdentifier) {
        self.out.push('(');
        self.ident(ty.value());
        self.out.push(')');
    }

    fn entry(&mut self, entry: &KdlEntry, depth: usize) {
        if let Some(name) = entry.name() {
            self.ident(name.value());
            self.out.push('=');
        }
        if let Some(ty) = entry.ty() {
            self.annotation(ty);
        }
        self.value(entry.value(), depth);
    }

    fn ident(&mut self, s: &str) {
        if is_bare_identifier(s) {
            self.out.push_str(s);
        } else {
            quoted(&mut self.out, s);
        }
    }

    fn value(&mut self, value: &KdlValue, depth: usize) {
        match value {
            KdlValue::String(s) if self.opts.multiline_strings && s.contains('\n') => {
                self.multiline(s, depth + 1);
            }
            KdlValue::String(s) => quoted(&mut self.out, s),
            KdlValue::Integer(i) => {
                let _ = write!(self.out, "{i}");
            }
            KdlValue::Float(f) => float(&mut self.out, *f),
            KdlValue::Bool(true) => self.out.push_str("#true"),
            KdlValue::Bool(false) => self.out.push_str("#false"),
            KdlValue::Null => self.out.push_str("#null"),
        }
    }

    /// A `"""` multiline string. Each line is indented one level past the
    /// node; the closing `"""` carries that indentation, which KDL strips.
    fn multiline(&mut self, s: &str, depth: usize) {
        let escape_quotes = s.contains("\"\"\"");
        self.out.push_str("\"\"\"\n");
        for line in s.split('\n') {
            if !line.is_empty() {
                self.indent(depth);
                multiline_line(&mut self.out, line, escape_quotes);
            }
            self.out.push('\n');
        }
        self.indent(depth);
        self.out.push_str("\"\"\"");
    }
}

fn float(out: &mut String, f: f64) {
    if f.is_nan() {
        out.push_str("#nan");
    } else if f.is_infinite() {
        out.push_str(if f > 0.0 { "#inf" } else { "#-inf" });
    } else {
        // `{:?}` always includes a `.` or an exponent, so it reads as a float.
        let _ = write!(out, "{f:?}");
    }
}

/// Characters KDL treats as newlines, besides `\n`.
fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Code points KDL forbids in documents (they must be escaped).
fn is_disallowed(c: char) -> bool {
    (c.is_control() && c != '\t')
        || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

fn escape_char(out: &mut String, c: char) -> bool {
    match c {
        '\\' => out.push_str("\\\\"),
        '\r' => out.push_str("\\r"),
        '\u{08}' => out.push_str("\\b"),
        '\u{0C}' => out.push_str("\\f"),
        c if is_newline(c) || is_disallowed(c) => {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        }
        _ => return false,
    }
    true
}

fn quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => {
                if !escape_char(out, c) {
                    out.push(c);
                }
            }
        }
    }
    out.push('"');
}

/// One line of a multiline string. Whitespace at the ends of a line is
/// escaped so dedenting and whitespace-only-line rules cannot eat it.
fn multiline_line(out: &mut String, line: &str, escape_quotes: bool) {
    let is_ws = |c: char| c == ' ' || c == '\t' || (c.is_whitespace() && !is_newline(c));
    let body_start = line.find(|c| !is_ws(c)).unwrap_or(line.len());
    let body_end = line.rfind(|c| !is_ws(c)).map_or(body_start, |i| {
        i + line[i..].chars().next().map_or(0, char::len_utf8)
    });
    // Leading whitespace is kept literally unless the line is all whitespace.
    let (lead, body, trail) = if body_start == line.len() {
        ("", "", line)
    } else {
        (
            &line[..body_start],
            &line[body_start..body_end],
            &line[body_end..],
        )
    };
    for c in lead.chars() {
        ws_char(out, c, false);
    }
    for c in body.chars() {
        match c {
            '"' if escape_quotes => out.push_str("\\\""),
            c => {
                if !escape_char(out, c) {
                    out.push(c);
                }
            }
        }
    }
    for c in trail.chars() {
        ws_char(out, c, true);
    }
}

fn ws_char(out: &mut String, c: char, escape: bool) {
    match c {
        ' ' if escape => out.push_str("\\s"),
        '\t' if escape => out.push_str("\\t"),
        ' ' | '\t' => out.push(c),
        c => {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        }
    }
}

/// Whether `s` can be written as a bare identifier. Deliberately
/// conservative: anything unusual gets quoted.
pub(crate) fn is_bare_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(s, "true" | "false" | "null" | "inf" | "-inf" | "nan") {
        return false;
    }
    let ok = |c: char| c.is_alphanumeric() || "-_.:+*!?$%&<>@^|~".contains(c);
    if !s
        .chars()
        .all(|c| ok(c) && !is_disallowed(c) && !c.is_whitespace())
    {
        return false;
    }
    if first.is_ascii_digit() {
        return false;
    }
    // `-1`, `+.5`, `.5` would read as numbers.
    let rest = s.strip_prefix(['-', '+']).unwrap_or(s);
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    !rest.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_identifiers() {
        for ok in [
            "a", "foo-bar", "-", "--", "a.b", "été", "x1", "-x", "$args", "@p",
        ] {
            assert!(is_bare_identifier(ok), "{ok}");
        }
        for no in [
            "", "1a", "-1", "+1", ".5", "-.5", "true", "inf", "a b", "a=b", "a\"", "#a", "a/b",
        ] {
            assert!(!is_bare_identifier(no), "{no}");
        }
    }

    #[test]
    fn floats() {
        let mut s = String::new();
        for f in [1.0, -0.5, 1e100, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            float(&mut s, f);
            s.push(' ');
        }
        assert_eq!(s, "1.0 -0.5 1e100 #inf #-inf #nan ");
    }
}
