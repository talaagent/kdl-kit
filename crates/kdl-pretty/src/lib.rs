//! A deterministic, comment-preserving KDL v2 formatter.
//!
//! kdl-rs's own `Display` and autoformat keep (or guess) source formatting
//! and collapse multiline strings into `\n` escapes (kdl-org/kdl-rs#77). This
//! printer re-renders every node from its parsed value and always produces
//! the same text for the same document: two-space indentation (by default),
//! one node per line, entries in order, bare identifiers where KDL allows
//! them, and strings with newlines as `"""` multiline strings.
//!
//! Comments and slashdashed (`/-`) nodes, entries and children blocks that
//! the parser kept in the document's format are re-emitted in normalized
//! positions, and a run of blank lines between nodes becomes one. Formatting
//! is idempotent: formatting the output again gives the same text.

use alloc::string::String;
use core::fmt::Write as _;

use kdl::{KdlDocument, KdlEntry, KdlError, KdlIdentifier, KdlNode, KdlValue};

extern crate alloc;

/// Formatter settings.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    /// Spaces per indentation level.
    pub indent: usize,
    /// Write strings that contain newlines as `"""` multiline strings.
    pub multiline_strings: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            indent: 2,
            multiline_strings: true,
        }
    }
}

impl Options {
    /// Sets [`indent`](Self::indent).
    #[must_use]
    pub fn indent(mut self, indent: usize) -> Self {
        self.indent = indent;
        self
    }

    /// Sets [`multiline_strings`](Self::multiline_strings).
    #[must_use]
    pub fn multiline_strings(mut self, multiline: bool) -> Self {
        self.multiline_strings = multiline;
        self
    }
}

/// Formats `doc` as KDL v2 text, one node per line with nested children
/// indented, keeping the comments its format holds.
///
/// kdl-rs drops a `// comment` that follows a children block's `}` on the
/// same line; [`format_str`] recovers it from the source text.
///
/// ```
/// let doc = kdl::KdlDocument::parse_v2("a   1 ;b { c x } // b").unwrap();
/// assert_eq!(kdl_pretty::format(&doc, &Default::default()), "a 1\nb {\n  c \"x\"\n}\n");
/// ```
pub fn format(doc: &KdlDocument, opts: &Options) -> String {
    let mut p = Printer::new(opts, doc.span().len());
    p.document(doc);
    p.out
}

/// Parses `src` as KDL v2 and formats it like [`format`].
///
/// ```
/// let src = "a 1 // one\nb { c #\"x\\y\"# } // b\n";
/// let out = kdl_pretty::format_str(src, &Default::default()).unwrap();
/// assert_eq!(out, "a 1 // one\nb {\n  c #\"x\\y\"#\n} // b\n");
/// ```
///
/// # Errors
///
/// The [`KdlError`] from parsing `src`.
pub fn format_str(src: &str, opts: &Options) -> Result<String, KdlError> {
    let doc = parse(src)?;
    let mut p = Printer::new(opts, src.len());
    p.document(&doc);
    Ok(p.out)
}

/// Formats `doc` with each top-level node on one line, children inline:
/// `a 1 { b 2; c 3; }`. Comments are dropped.
pub fn format_compact(doc: &KdlDocument) -> String {
    let opts = Options::default().multiline_strings(false);
    let mut p = Printer::new(&opts, doc.span().len());
    p.compact(doc.nodes(), 0);
    p.out
}

/// Parses `src`, then restores the comments after a `}` that kdl-rs
/// consumes without keeping: a `// comment` (taken as the node's
/// terminator) and `/* comments */` (taken as trailing node space).
fn parse(src: &str) -> Result<KdlDocument, KdlError> {
    let mut doc = KdlDocument::parse_v2(src)?;
    recover_comments(&mut doc, src);
    Ok(doc)
}

fn recover_comments(doc: &mut KdlDocument, src: &str) {
    for node in doc.nodes_mut() {
        let start = node.span().offset();
        let end = start + node.span().len();
        let has_children = node.children().is_some();
        if let Some(fmt) = node.format_mut() {
            if has_children && let Some(text) = src.get(start..end) {
                let comments = trailing_block_comments(text);
                if !comments.is_empty() {
                    fmt.before_terminator.push(' ');
                    fmt.before_terminator.push_str(&comments);
                }
            }
            if fmt.terminator.is_empty()
                && let Some(rest) = src.get(end..)
                && rest.starts_with("//")
            {
                let len = rest.find(is_any_newline).unwrap_or(rest.len());
                fmt.terminator = String::from(&rest[..len]);
                fmt.terminator.push('\n');
            }
        }
        if let Some(children) = node.children_mut() {
            recover_comments(children, src);
        }
    }
}

/// The `/* … */` comments at the end of `text`, in order.
fn trailing_block_comments(text: &str) -> String {
    let mut found = alloc::vec::Vec::new();
    let mut t = text.trim_end();
    while t.ends_with("*/") {
        let bytes = t.as_bytes();
        let mut i = bytes.len();
        let mut depth = 0usize;
        let start = loop {
            if i < 2 {
                return String::new();
            }
            if bytes[..i].ends_with(b"*/") {
                depth += 1;
                i -= 2;
            } else if bytes[..i].ends_with(b"/*") {
                depth -= 1;
                i -= 2;
                if depth == 0 {
                    break i;
                }
            } else {
                i -= 1;
            }
        };
        found.push(&t[start..]);
        t = t[..start].trim_end();
    }
    found.reverse();
    found.join(" ")
}

struct Printer<'o> {
    out: String,
    opts: &'o Options,
    /// The output's last line has content and no `\n` yet.
    open: bool,
    /// The next node goes on the open line, after a `/* */` comment.
    glue: bool,
    /// The source had a blank line before the next item.
    blank: bool,
    /// Nothing has been printed in the current block yet.
    first: bool,
}

impl<'o> Printer<'o> {
    fn new(opts: &'o Options, capacity: usize) -> Self {
        Self {
            out: String::with_capacity(capacity + capacity / 8 + 16),
            opts,
            open: false,
            glue: false,
            blank: false,
            first: true,
        }
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth * self.opts.indent {
            self.out.push(' ');
        }
    }

    /// A space before the next token, unless one is already there.
    fn sep(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with([' ', '\n', '(']) {
            self.out.push(' ');
        }
    }

    fn end_line(&mut self) {
        if self.open {
            let len = self.out.trim_end_matches(' ').len();
            self.out.truncate(len);
            self.out.push('\n');
            self.open = false;
        }
        self.glue = false;
    }

    /// Starts a node or own-line comment at `depth`.
    fn item(&mut self, depth: usize) {
        if self.glue && self.open {
            self.sep();
        } else {
            self.end_line();
            if self.blank && !self.first {
                self.out.push('\n');
            }
            self.indent(depth);
            self.open = true;
        }
        self.glue = false;
        self.blank = false;
        self.first = false;
    }

    fn document(&mut self, doc: &KdlDocument) {
        self.block(doc, 0, true);
        self.end_line();
    }

    /// The nodes of `doc` and the comments around them. `sol`: the source
    /// is at the start of a line (not right after a `{`).
    fn block(&mut self, doc: &KdlDocument, depth: usize, sol: bool) {
        self.first = true;
        self.blank = false;
        let mut sol = sol;
        if let Some(f) = doc.format() {
            sol = self.trivia(&f.leading, depth, sol);
        }
        for node in doc.nodes() {
            sol = self.node(node, depth, sol, false);
        }
        if let Some(f) = doc.format() {
            self.trivia(&f.trailing, depth, sol);
        }
    }

    /// One node with its leading comments and trailing comment. Returns
    /// whether the source is at the start of a line after it.
    fn node(&mut self, node: &KdlNode, depth: usize, sol: bool, slashdash: bool) -> bool {
        let f = node.format();
        if let Some(f) = f {
            self.trivia(&f.leading, depth, sol);
        }
        self.item(depth);
        if slashdash {
            self.out.push_str("/-");
        }
        if let Some(ty) = node.ty() {
            self.annotation(ty, f.map(|f| (&*f.before_ty_name, &*f.after_ty_name)));
            self.inline(f.map_or("", |f| &f.after_ty), depth);
        }
        self.ident(node.name().value());
        for entry in node.entries() {
            self.entry(entry, depth, false);
        }
        self.inline(f.map_or("", |f| &f.before_children), depth);
        if let Some(children) = node.children() {
            self.children(children, depth, false);
        }
        self.inline(f.map_or("", |f| &f.before_terminator), depth);
        let term = f.map_or("", |f| &f.terminator);
        let sol = if let Some(i) = term.find("//") {
            self.sep();
            self.out.push_str(term[i..].trim_end());
            self.end_line();
            true
        } else {
            !term.starts_with(';')
        };
        // kdl-rs's parser leaves `trailing` empty; `KdlNode::new` sets it
        // to "\n", which must not read as a blank line.
        match f {
            Some(f) if !f.trailing.is_empty() => self.trivia(&f.trailing, depth, false),
            _ => sol,
        }
    }

    fn children(&mut self, children: &KdlDocument, depth: usize, slashdash: bool) {
        self.sep();
        if slashdash {
            self.out.push_str("/-");
        }
        self.out.push('{');
        let empty = self.out.len();
        self.block(children, depth + 1, false);
        if self.out.len() > empty {
            self.end_line();
            self.indent(depth);
        }
        self.out.push('}');
        self.open = true;
        self.glue = false;
        self.blank = false;
        self.first = false;
    }

    fn annotation(&mut self, ty: &KdlIdentifier, trivia: Option<(&str, &str)>) {
        let (before, after) = trivia.unwrap_or_default();
        self.out.push('(');
        self.inline(before, 0);
        self.ident(ty.value());
        self.inline(after, 0);
        self.out.push(')');
    }

    fn entry(&mut self, entry: &KdlEntry, depth: usize, slashdash: bool) {
        let f = entry.format();
        let field = |get: fn(&kdl::KdlEntryFormat) -> &str| f.map_or("", get);
        self.inline(field(|f| &f.leading), depth);
        self.sep();
        if slashdash {
            self.out.push_str("/-");
        }
        if let Some(name) = entry.name() {
            self.ident(name.value());
            self.inline(field(|f| &f.after_key), depth);
            self.out.push('=');
            self.inline(field(|f| &f.after_eq), depth);
        }
        if let Some(ty) = entry.ty() {
            self.annotation(ty, f.map(|f| (&*f.before_ty_name, &*f.after_ty_name)));
            self.inline(field(|f| &f.after_ty), depth);
        }
        let raw = f.is_some_and(|f| f.value_repr.starts_with('#'));
        match entry.value() {
            KdlValue::String(s) if raw => self.raw(s, depth + 1),
            value => self.value(value, depth),
        }
        self.inline(field(|f| &f.trailing), depth);
    }

    /// Comments and slashdashes between nodes, at `depth`. Returns whether
    /// the source is at the start of a line after them.
    fn trivia(&mut self, text: &str, depth: usize, mut sol: bool) -> bool {
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            if rest.starts_with("//") {
                let len = rest.find(is_any_newline).unwrap_or(rest.len());
                if sol || !self.open {
                    self.item(depth);
                } else {
                    self.sep();
                }
                self.out.push_str(rest[..len].trim_end());
                self.end_line();
                sol = true;
                rest = skip_newline(&rest[len..]);
            } else if rest.starts_with("/*") {
                let len = block_comment_len(rest);
                if sol || !self.open {
                    self.item(depth);
                    self.out.push_str(&rest[..len]);
                    self.glue = true;
                } else {
                    self.sep();
                    self.out.push_str(&rest[..len]);
                }
                sol = false;
                rest = &rest[len..];
            } else if let Some(r) = rest.strip_prefix("/-") {
                let r = r.trim_start();
                return match parse(r) {
                    Ok(doc) if doc.nodes().len() == 1 => {
                        let sol = self.node(&doc.nodes()[0], depth, sol, true);
                        let trailing = doc.format().map_or("", |f| &f.trailing);
                        self.trivia(trailing, depth, sol)
                    }
                    _ => {
                        self.item(depth);
                        self.out.push_str("/-");
                        self.out.push_str(r.trim_end());
                        self.end_line();
                        true
                    }
                };
            } else if is_any_newline(c) {
                if sol {
                    self.blank = true;
                }
                sol = true;
                self.glue = false;
                rest = skip_newline(rest);
            } else {
                // Whitespace and line continuations.
                rest = &rest[c.len_utf8()..];
            }
        }
        sol
    }

    /// Comments, line continuations and slashdashes inside a node.
    fn inline(&mut self, text: &str, depth: usize) {
        let mut rest = text;
        while let Some(c) = rest.chars().next() {
            if rest.starts_with("/*") {
                let len = block_comment_len(rest);
                self.sep();
                self.out.push_str(&rest[..len]);
                self.out.push(' ');
                rest = &rest[len..];
            } else if rest.starts_with("//") {
                // Only after a `\` line continuation.
                let len = rest.find(is_any_newline).unwrap_or(rest.len());
                self.sep();
                self.out.push_str("\\ ");
                self.out.push_str(rest[..len].trim_end());
                self.out.push('\n');
                self.indent(depth + 1);
                rest = skip_newline(&rest[len..]);
            } else if let Some(r) = rest.strip_prefix("/-") {
                self.slashdash_inline(r.trim_start(), depth);
                return;
            } else {
                rest = &rest[c.len_utf8()..];
            }
        }
    }

    /// A slashdashed entry or children block, and the trivia after it.
    fn slashdash_inline(&mut self, r: &str, depth: usize) {
        let mut src = String::with_capacity(r.len() + 2);
        src.push_str("n ");
        src.push_str(r);
        match parse(&src) {
            Ok(doc) if doc.nodes().len() == 1 => {
                let node = &doc.nodes()[0];
                let f = node.format();
                for entry in node.entries() {
                    self.entry(entry, depth, true);
                }
                self.inline(f.map_or("", |f| &f.before_children), depth);
                if let Some(children) = node.children() {
                    self.children(children, depth, true);
                }
                self.inline(f.map_or("", |f| &f.before_terminator), depth);
            }
            _ => {
                self.sep();
                self.out.push_str("/-");
                self.out.push_str(r.trim_end());
            }
        }
    }

    fn compact(&mut self, nodes: &[KdlNode], depth: usize) {
        for node in nodes {
            if depth > 0 {
                self.out.push(' ');
                self.compact_node(node, depth);
                self.out.push(';');
            } else {
                self.compact_node(node, depth);
                self.out.push('\n');
            }
        }
    }

    fn compact_node(&mut self, node: &KdlNode, depth: usize) {
        if let Some(ty) = node.ty() {
            self.annotation(ty, None);
        }
        self.ident(node.name().value());
        for entry in node.entries() {
            self.out.push(' ');
            if let Some(name) = entry.name() {
                self.ident(name.value());
                self.out.push('=');
            }
            if let Some(ty) = entry.ty() {
                self.annotation(ty, None);
            }
            self.value(entry.value(), depth);
        }
        if let Some(children) = node.children().filter(|c| !c.nodes().is_empty()) {
            self.out.push_str(" {");
            self.compact(children.nodes(), depth + 1);
            self.out.push_str(" }");
        }
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

    /// A string that was raw in the source: raw again (`#"…"#`, or
    /// `#"""…"""#` re-indented to `depth`) when a raw string can hold it.
    fn raw(&mut self, s: &str, depth: usize) {
        let hashes = raw_hashes(s);
        if !s.contains('\n') && raw_line_ok(s) {
            self.hashes(hashes);
            self.out.push('"');
            self.out.push_str(s);
            self.out.push('"');
            self.hashes(hashes);
        } else if self.opts.multiline_strings
            && s.contains('\n')
            && s.split('\n')
                .all(|l| raw_line_ok(l) && (l.is_empty() || !l.chars().all(char::is_whitespace)))
        {
            self.hashes(hashes);
            self.out.push_str("\"\"\"\n");
            for line in s.split('\n') {
                if !line.is_empty() {
                    self.indent(depth);
                    self.out.push_str(line);
                }
                self.out.push('\n');
            }
            self.indent(depth);
            self.out.push_str("\"\"\"");
            self.hashes(hashes);
        } else {
            let value = KdlValue::String(String::from(s));
            self.value(&value, depth - 1);
        }
    }

    fn hashes(&mut self, n: usize) {
        for _ in 0..n {
            self.out.push('#');
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

/// Fewest `#`s that no `"` in `s` is followed by, so `s` can't end the
/// raw string early.
fn raw_hashes(s: &str) -> usize {
    let mut most = 0;
    for (i, _) in s.match_indices('"') {
        let run = s[i + 1..].bytes().take_while(|&b| b == b'#').count();
        most = most.max(run);
    }
    most + 1
}

/// Whether a raw string line can hold `s` verbatim.
fn raw_line_ok(s: &str) -> bool {
    !s.chars().any(|c| is_disallowed(c) || is_newline(c))
}

fn is_any_newline(c: char) -> bool {
    c == '\n' || is_newline(c)
}

fn skip_newline(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("\r\n") {
        rest
    } else {
        let mut chars = s.chars();
        chars.next();
        chars.as_str()
    }
}

/// Length of the (possibly nested) `/* … */` comment at the start of `s`.
fn block_comment_len(s: &str) -> usize {
    let mut depth = 0usize;
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i..].starts_with(b"/*") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b"*/") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    s.len()
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

/// Whether `s` can be written as a bare identifier.
/// Deliberately
/// conservative: anything unusual gets quoted.
pub fn is_bare_identifier(s: &str) -> bool {
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
