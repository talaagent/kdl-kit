// Derived from club-kdl 0.17.0 `src/parse.rs`
// (https://github.com/chronista-club/club-kdl),
// Copyright 2025-2026 Mako (mito@chronista.club),
// licensed under the MIT license or the Apache License, Version 2.0, at
// your option. Changes: spans on every node, name, type, entry, key, value
// and children block; structured errors with byte offsets and codes; a
// configurable depth cap; a slashdashed entry no longer needs whitespace
// before its `/-` (`node "a"/-1`), as the KDL 2.0 grammar allows
// (`node-space* (node-space | slashdash) node-prop-or-arg`).

//! The KDL v2 parser: one pass over the bytes with an ASCII fast path,
//! decoding UTF-8 only on non-ASCII bytes. Strings that need no unescaping
//! (identifiers, plain quoted strings, single-line raw strings) are borrowed
//! from the source.

use alloc::borrow::Cow;
use alloc::string::String;

use crate::error::{Error, ErrorKind};
use crate::{Document, Entry, Ident, Node, ParseOptions, Span, Value};

/// Boxing the error keeps `PResult<bool>` and friends small on the hot
/// paths.
type PResult<T> = Result<T, Box<Error>>;

pub(crate) fn parse<'a>(src: &'a str, opts: &ParseOptions) -> Result<Document<'a>, Error> {
    let mut p = Parser {
        src,
        b: src.as_bytes(),
        pos: 0,
        depth: 0,
        max_depth: opts.max_depth,
        base: opts.base,
    };
    if src.starts_with('\u{FEFF}') {
        p.pos = 3;
    }
    let nodes = p.nodes(None).map_err(|e| *e)?;
    Ok(Document::new(nodes, Span::new(opts.base, src.len())))
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    depth: usize,
    max_depth: usize,
    base: usize,
}

/// ASCII bytes that may appear in an identifier string.
const fn ident_table() -> [bool; 128] {
    let mut t = [false; 128];
    let mut i = 0x21;
    while i < 0x7F {
        t[i] = true;
        i += 1;
    }
    let banned = b"\\/(){};[]\"#=";
    let mut j = 0;
    while j < banned.len() {
        t[banned[j] as usize] = false;
        j += 1;
    }
    t
}
static IDENT_ASCII: [bool; 128] = ident_table();

#[inline]
fn is_unicode_space(c: char) -> bool {
    matches!(
        c,
        '\t' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

#[inline]
fn is_newline_char(c: char) -> bool {
    matches!(
        c,
        '\r' | '\n' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

#[inline]
fn is_disallowed(c: char) -> bool {
    matches!(
        c,
        '\u{0000}'..='\u{0008}'
            | '\u{000E}'..='\u{001F}'
            | '\u{007F}'
            | '\u{200E}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

#[inline]
fn is_ident_char(c: char) -> bool {
    if c.is_ascii() {
        IDENT_ASCII[c as usize]
    } else {
        !is_unicode_space(c) && !is_newline_char(c) && !is_disallowed(c)
    }
}

impl<'a> Parser<'a> {
    // ------------------------------------------------------------------
    // Low-level helpers

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    #[inline]
    fn peek_at(&self, off: usize) -> Option<u8> {
        self.b.get(self.pos + off).copied()
    }

    #[inline]
    fn starts_with(&self, s: &[u8]) -> bool {
        self.b[self.pos..].starts_with(s)
    }

    #[inline]
    fn eof(&self) -> bool {
        self.pos >= self.b.len()
    }

    #[inline]
    fn peek_char(&self) -> Option<char> {
        match self.peek() {
            None => None,
            Some(b) if b < 0x80 => Some(b as char),
            Some(_) => self.src[self.pos..].chars().next(),
        }
    }

    #[inline]
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.base + start, end - start)
    }

    #[cold]
    #[inline(never)]
    fn err<T>(&self, kind: ErrorKind, msg: impl Into<Cow<'static, str>>) -> PResult<T> {
        Err(self.error_at(kind, self.pos, msg))
    }

    /// An error pointing at the character at `pos` (empty at the end).
    #[cold]
    #[inline(never)]
    fn error_at(
        &self,
        kind: ErrorKind,
        pos: usize,
        msg: impl Into<Cow<'static, str>>,
    ) -> Box<Error> {
        let pos = pos.min(self.src.len());
        let len = self.src[pos..].chars().next().map_or(0, char::len_utf8);
        Box::new(Error::new(kind, self.span(pos, pos + len), msg))
    }

    /// Consume one newline sequence if present.
    #[inline]
    fn newline(&mut self) -> bool {
        match self.peek() {
            Some(b'\n' | 0x0B | 0x0C) => {
                self.pos += 1;
                true
            }
            Some(b'\r') => {
                self.pos += if self.peek_at(1) == Some(b'\n') { 2 } else { 1 };
                true
            }
            Some(b) if b >= 0x80 => match self.peek_char() {
                Some(c) if is_newline_char(c) => {
                    self.pos += c.len_utf8();
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    #[inline]
    fn at_newline(&self) -> bool {
        match self.peek() {
            Some(b'\n' | b'\r' | 0x0B | 0x0C) => true,
            Some(b) if b >= 0x80 => self.peek_char().is_some_and(is_newline_char),
            _ => false,
        }
    }

    /// Consume one unicode-space character if present.
    #[inline]
    fn unicode_space(&mut self) -> bool {
        match self.peek() {
            Some(b' ' | b'\t') => {
                self.pos += 1;
                true
            }
            Some(b) if b >= 0x80 => match self.peek_char() {
                Some(c) if is_unicode_space(c) => {
                    self.pos += c.len_utf8();
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// `'/*' commented-block` (nestable).
    fn multi_line_comment(&mut self) -> PResult<bool> {
        if !self.starts_with(b"/*") {
            return Ok(false);
        }
        let start = self.pos;
        self.pos += 2;
        let mut depth = 1usize;
        while depth > 0 {
            match self.peek() {
                None => {
                    return Err(self.error_at(
                        ErrorKind::Unterminated,
                        start,
                        "unterminated block comment",
                    ));
                }
                Some(b'*') if self.peek_at(1) == Some(b'/') => {
                    depth -= 1;
                    self.pos += 2;
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    depth += 1;
                    self.pos += 2;
                }
                Some(_) => self.advance_char_checked()?,
            }
        }
        Ok(true)
    }

    /// Advance past one character, rejecting disallowed code points.
    #[inline]
    fn advance_char_checked(&mut self) -> PResult<()> {
        let Some(c) = self.peek_char() else {
            return Ok(());
        };
        if is_disallowed(c) {
            return self.err(
                ErrorKind::DisallowedChar,
                format!("disallowed character U+{:04X}", c as u32),
            );
        }
        self.pos += c.len_utf8();
        Ok(())
    }

    /// `ws := unicode-space | multi-line-comment`
    #[inline]
    fn ws(&mut self) -> PResult<bool> {
        if self.unicode_space() {
            return Ok(true);
        }
        self.multi_line_comment()
    }

    /// `'//' ^newline* (newline | eof)`
    fn single_line_comment(&mut self) -> PResult<bool> {
        if !self.starts_with(b"//") {
            return Ok(false);
        }
        self.pos += 2;
        loop {
            // `"` and `\` are ordinary in a comment; the chunk skip just
            // treats them as "look closer", which the byte loop accepts.
            self.pos = skip_plain_ascii(self.b, self.pos);
            match self.peek() {
                Some(b) if (0x20..0x7F).contains(&b) || b == b'\t' => self.pos += 1,
                None => break,
                _ if self.at_newline() => break,
                _ => self.advance_char_checked()?,
            }
        }
        self.newline();
        Ok(true)
    }

    /// `escline := '\\' ws* (single-line-comment | newline | eof)`
    fn escline(&mut self) -> PResult<bool> {
        if self.peek() != Some(b'\\') {
            return Ok(false);
        }
        let save = self.pos;
        self.pos += 1;
        while self.ws()? {}
        if self.single_line_comment()? || self.newline() || self.eof() {
            Ok(true)
        } else {
            self.pos = save;
            Ok(false)
        }
    }

    /// `node-space*`; returns whether anything was consumed.
    fn node_space(&mut self) -> PResult<bool> {
        let start = self.pos;
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.pos += 1;
            }
            if self.ws()? || self.escline()? {
                continue;
            }
            break;
        }
        Ok(self.pos != start)
    }

    /// `line-space*`
    fn line_space(&mut self) -> PResult<()> {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.pos += 1;
            }
            if self.newline() || self.node_space()? || self.single_line_comment()? {
                continue;
            }
            return Ok(());
        }
    }

    // ------------------------------------------------------------------
    // Nodes

    /// The nodes of the document, or of the children block whose `{` is at
    /// `open`.
    fn nodes(&mut self, open: Option<usize>) -> PResult<Vec<Node<'a>>> {
        let in_children = open.is_some();
        let mut nodes = Vec::new();
        loop {
            self.line_space()?;
            match self.peek() {
                None => {
                    if let Some(open) = open {
                        return Err(self.error_at(
                            ErrorKind::Unterminated,
                            open,
                            "unterminated children block, expected `}`",
                        ));
                    }
                    return Ok(nodes);
                }
                Some(b'}') => {
                    if in_children {
                        return Ok(nodes);
                    }
                    return self.err(ErrorKind::Unexpected, "unexpected `}`");
                }
                _ => {}
            }
            if self.starts_with(b"/-") {
                self.pos += 2;
                self.line_space()?;
                self.node(in_children)?;
            } else {
                let node = self.node(in_children)?;
                nodes.push(node);
            }
        }
    }

    fn node(&mut self, in_children: bool) -> PResult<Node<'a>> {
        let start = self.pos;
        let ty = if self.peek() == Some(b'(') {
            let t = self.type_annotation()?;
            self.node_space()?;
            Some(t)
        } else {
            None
        };
        let name_start = self.pos;
        let Some(name) = self.string()? else {
            return self.err(ErrorKind::Unexpected, "expected a node name");
        };
        let name = Ident::new(name, self.span(name_start, self.pos));
        let mut end = self.pos;
        let mut entries = Vec::new();
        let mut children = None;

        // Once any children block (slashdashed or not) appears, no more
        // entries are allowed; once a real one appears, only slashdashed
        // blocks may follow.
        let mut entries_closed = false;
        loop {
            let spaced = self.node_space()?;
            match self.peek() {
                None | Some(b';' | b'}') => break,
                _ if self.at_newline() || self.starts_with(b"//") => break,
                Some(b'{') => {
                    if children.is_some() {
                        return self.err(
                            ErrorKind::ChildrenOrder,
                            "a node may only have one children block",
                        );
                    }
                    children = Some(self.children_block()?);
                    end = self.pos;
                    entries_closed = true;
                }
                _ if self.starts_with(b"/-") => {
                    self.pos += 2;
                    self.line_space()?;
                    if self.peek() == Some(b'{') {
                        self.children_block()?;
                        entries_closed = true;
                    } else {
                        if entries_closed {
                            return self.err(
                                ErrorKind::ChildrenOrder,
                                "entries must come before the children block",
                            );
                        }
                        self.entry()?;
                    }
                }
                _ => {
                    if entries_closed {
                        return self.err(
                            ErrorKind::ChildrenOrder,
                            "entries must come before the children block",
                        );
                    }
                    if !spaced {
                        return self.err(ErrorKind::Unexpected, "expected whitespace before entry");
                    }
                    entries.push(self.entry()?);
                    end = self.pos;
                }
            }
        }

        // node-terminator
        match self.peek() {
            None => {}
            Some(b';') => self.pos += 1,
            Some(b'}') => {
                if !in_children {
                    return self.err(ErrorKind::Unexpected, "unexpected `}`");
                }
            }
            _ => {
                if !self.newline() && !self.single_line_comment()? {
                    return self.err(ErrorKind::Unexpected, "expected a node terminator");
                }
            }
        }
        Ok(Node::new(
            ty,
            name,
            entries,
            children,
            self.span(start, end),
        ))
    }

    fn children_block(&mut self) -> PResult<Document<'a>> {
        let start = self.pos;
        if self.depth >= self.max_depth {
            return self.err(
                ErrorKind::TooDeep,
                format!("children nested more than {} deep", self.max_depth),
            );
        }
        self.pos += 1;
        self.depth += 1;
        let nodes = self.nodes(Some(start))?;
        self.depth -= 1;
        // nodes(Some(_)) only returns Ok when positioned at '}'
        self.pos += 1;
        Ok(Document::new(nodes, self.span(start, self.pos)))
    }

    /// `type := '(' node-space* string node-space* ')'`
    fn type_annotation(&mut self) -> PResult<Ident<'a>> {
        self.pos += 1; // '('
        self.node_space()?;
        let start = self.pos;
        let Some(s) = self.string()? else {
            return self.err(ErrorKind::Unexpected, "expected a type name");
        };
        let ident = Ident::new(s, self.span(start, self.pos));
        self.node_space()?;
        if self.peek() != Some(b')') {
            return self.err(ErrorKind::Unexpected, "expected `)`");
        }
        self.pos += 1;
        Ok(ident)
    }

    /// `prop | value`
    fn entry(&mut self) -> PResult<Entry<'a>> {
        let start = self.pos;
        if self.peek() == Some(b'(') {
            let ty = self.type_annotation()?;
            self.node_space()?;
            let value_start = self.pos;
            let value = self.value_body()?;
            let value_span = self.span(value_start, self.pos);
            return Ok(Entry::new(
                None,
                Some(ty),
                value,
                value_span,
                self.span(start, self.pos),
            ));
        }
        if let Some(s) = self.string()? {
            let after = self.pos;
            self.node_space()?;
            if self.peek() == Some(b'=') {
                let key = Ident::new(s, self.span(start, after));
                self.pos += 1;
                self.node_space()?;
                let ty = if self.peek() == Some(b'(') {
                    let t = self.type_annotation()?;
                    self.node_space()?;
                    Some(t)
                } else {
                    None
                };
                let value_start = self.pos;
                let value = self.value_body()?;
                let value_span = self.span(value_start, self.pos);
                return Ok(Entry::new(
                    Some(key),
                    ty,
                    value,
                    value_span,
                    self.span(start, self.pos),
                ));
            }
            self.pos = after;
            let span = self.span(start, after);
            return Ok(Entry::new(None, None, Value::String(s), span, span));
        }
        let value = self.value_body()?;
        let span = self.span(start, self.pos);
        Ok(Entry::new(None, None, value, span, span))
    }

    /// `string | number | keyword`
    fn value_body(&mut self) -> PResult<Value<'a>> {
        if let Some(s) = self.string()? {
            return Ok(Value::String(s));
        }
        match self.peek() {
            Some(b'#') => self.keyword(),
            Some(b'0'..=b'9') => self.number(),
            Some(b'+' | b'-') if matches!(self.peek_at(1), Some(b'0'..=b'9')) => self.number(),
            _ => self.err(ErrorKind::Unexpected, "expected a value"),
        }
    }

    fn keyword(&mut self) -> PResult<Value<'a>> {
        const KEYWORDS: [(&[u8], Value<'static>); 6] = [
            (b"#true", Value::Bool(true)),
            (b"#false", Value::Bool(false)),
            (b"#null", Value::Null),
            (b"#inf", Value::Float(f64::INFINITY)),
            (b"#-inf", Value::Float(f64::NEG_INFINITY)),
            (b"#nan", Value::Float(f64::NAN)),
        ];
        for (kw, v) in KEYWORDS {
            if self.starts_with(kw) {
                let save = self.pos;
                self.pos += kw.len();
                if self.peek_char().is_some_and(is_ident_char) {
                    self.pos = save;
                    break;
                }
                return Ok(v);
            }
        }
        self.err(ErrorKind::InvalidKeyword, "unknown keyword")
    }

    // ------------------------------------------------------------------
    // Numbers

    fn number(&mut self) -> PResult<Value<'a>> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let radix = match (self.peek(), self.peek_at(1)) {
            (Some(b'0'), Some(b'x')) => 16,
            (Some(b'0'), Some(b'o')) => 8,
            (Some(b'0'), Some(b'b')) => 2,
            _ => 10,
        };
        let range = |p: &Self| p.error_at(ErrorKind::InvalidNumber, start, "integer out of range");
        let value = if radix != 10 {
            self.pos += 2;
            let digits_start = self.pos;
            let is_digit = |b: u8| match radix {
                16 => b.is_ascii_hexdigit(),
                8 => (b'0'..=b'7').contains(&b),
                _ => b == b'0' || b == b'1',
            };
            if !self.peek().is_some_and(is_digit) {
                return Err(Box::new(Error::new(
                    ErrorKind::InvalidNumber,
                    self.span(start, self.pos),
                    "expected a digit after the radix prefix",
                )));
            }
            while self.peek().is_some_and(|b| is_digit(b) || b == b'_') {
                self.pos += 1;
            }
            let digits = strip_underscores(&self.src[digits_start..self.pos]);
            let negative = self.b[start] == b'-';
            let magnitude = u128::from_str_radix(&digits, radix).map_err(|_| range(self))?;
            Value::Integer(to_i128(magnitude, negative).ok_or_else(|| range(self))?)
        } else {
            self.digits()?;
            let mut is_float = false;
            if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
                self.digits()?;
                is_float = true;
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                let save = self.pos;
                self.pos += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                if self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.digits()?;
                    is_float = true;
                } else {
                    self.pos = save;
                }
            }
            let text = strip_underscores(&self.src[start..self.pos]);
            if is_float {
                Value::Float(
                    text.parse::<f64>().map_err(|_| {
                        self.error_at(ErrorKind::InvalidNumber, start, "invalid float")
                    })?,
                )
            } else {
                Value::Integer(text.parse::<i128>().map_err(|_| range(self))?)
            }
        };
        if self.peek_char().is_some_and(is_ident_char) {
            return self.err(ErrorKind::InvalidNumber, "invalid character in number");
        }
        Ok(value)
    }

    /// `digit (digit | '_')*`
    fn digits(&mut self) -> PResult<()> {
        if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
            return self.err(ErrorKind::InvalidNumber, "expected a digit");
        }
        while self.peek().is_some_and(|b| b.is_ascii_digit() || b == b'_') {
            self.pos += 1;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Strings

    /// Any string form, or `None` if the input does not start with one.
    fn string(&mut self) -> PResult<Option<Cow<'a, str>>> {
        match self.peek() {
            Some(b'"') => self.quoted_string().map(Some),
            Some(b'#') => {
                let mut i = 1;
                while self.peek_at(i) == Some(b'#') {
                    i += 1;
                }
                if self.peek_at(i) == Some(b'"') {
                    self.raw_string().map(Some)
                } else {
                    Ok(None)
                }
            }
            Some(_) => self.identifier(),
            None => Ok(None),
        }
    }

    fn identifier(&mut self) -> PResult<Option<Cow<'a, str>>> {
        let start = self.pos;
        let first = match self.peek_char() {
            Some(c) if is_ident_char(c) => c,
            _ => return Ok(None),
        };
        if first.is_ascii_digit() {
            return Ok(None);
        }
        // A sign followed by a digit is a number.
        if (first == '+' || first == '-') && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
            return Ok(None);
        }
        loop {
            match self.peek() {
                Some(b) if b < 0x80 => {
                    if IDENT_ASCII[b as usize] {
                        self.pos += 1;
                    } else if is_disallowed(b as char) {
                        return self.err(
                            ErrorKind::DisallowedChar,
                            format!("disallowed character U+{b:04X}"),
                        );
                    } else {
                        break;
                    }
                }
                Some(_) => {
                    let Some(c) = self.peek_char() else { break };
                    if is_ident_char(c) {
                        self.pos += c.len_utf8();
                    } else if is_disallowed(c) {
                        return self.err(
                            ErrorKind::DisallowedChar,
                            format!("disallowed character U+{:04X}", c as u32),
                        );
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        let s = &self.src[start..self.pos];
        let bytes = s.as_bytes();
        // dotted-ident: `.` (optionally after a sign) must not be followed
        // by a digit.
        let after_sign = usize::from(matches!(bytes[0], b'+' | b'-'));
        if bytes.get(after_sign) == Some(&b'.')
            && bytes
                .get(after_sign + 1)
                .is_some_and(|b| b.is_ascii_digit())
        {
            return Err(self.error_at(
                ErrorKind::InvalidIdentifier,
                start,
                "an identifier cannot look like a number",
            ));
        }
        if matches!(s, "true" | "false" | "null" | "inf" | "-inf" | "nan") {
            let e = Error::new(
                ErrorKind::InvalidIdentifier,
                self.span(start, self.pos),
                format!("bare `{s}` is not a value"),
            )
            .with_help("write a keyword with `#` (`#true`, `#null`), or quote it as a string");
            return Err(Box::new(e));
        }
        Ok(Some(Cow::Borrowed(s)))
    }

    fn quoted_string(&mut self) -> PResult<Cow<'a, str>> {
        let start = self.pos;
        if self.starts_with(b"\"\"\"") {
            return self.multi_line_quoted(start);
        }
        self.pos += 1;
        let body_start = self.pos;
        let mut has_escape = false;
        loop {
            self.pos = skip_plain_ascii(self.b, self.pos);
            match self.peek() {
                Some(b) if (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' => {
                    self.pos += 1;
                }
                None => {
                    return Err(self.error_at(
                        ErrorKind::Unterminated,
                        start,
                        "unterminated string",
                    ));
                }
                Some(b'"') => break,
                Some(b'\\') => {
                    has_escape = true;
                    self.pos += 1;
                    if self.eof() {
                        return Err(self.error_at(
                            ErrorKind::Unterminated,
                            start,
                            "unterminated string",
                        ));
                    }
                    // A ws-escape swallows any run of whitespace and
                    // newlines; other escapes are validated in `unescape`.
                    let mut ws_escape = false;
                    while self.newline() || self.unicode_space() {
                        ws_escape = true;
                    }
                    if !ws_escape {
                        self.advance_char_checked()?;
                    }
                }
                _ => {
                    if self.at_newline() {
                        let e = Error::new(
                            ErrorKind::InvalidString,
                            self.span(start, self.pos),
                            "unexpected newline in a single-line string",
                        )
                        .with_help(
                            "close the string, or write a multi-line string as \"\"\"…\"\"\"",
                        );
                        return Err(Box::new(e));
                    }
                    self.advance_char_checked()?;
                }
            }
        }
        let body = &self.src[body_start..self.pos];
        self.pos += 1; // closing quote
        if has_escape {
            unescape(body).map(Cow::Owned).map_err(|(off, msg)| {
                self.error_at(ErrorKind::InvalidEscape, body_start + off, msg)
            })
        } else {
            Ok(Cow::Borrowed(body))
        }
    }

    fn multi_line_quoted(&mut self, start: usize) -> PResult<Cow<'a, str>> {
        self.pos += 3;
        if !self.newline() {
            return self.err(
                ErrorKind::InvalidString,
                "a multi-line string needs a newline after \"\"\"",
            );
        }
        let body_start = self.pos;
        loop {
            match self.peek() {
                None => {
                    return Err(self.error_at(
                        ErrorKind::Unterminated,
                        start,
                        "unterminated multi-line string",
                    ));
                }
                Some(b'"') if self.starts_with(b"\"\"\"") => break,
                Some(b'\\') => {
                    self.pos += 1;
                    if self.eof() {
                        return Err(self.error_at(
                            ErrorKind::Unterminated,
                            start,
                            "unterminated multi-line string",
                        ));
                    }
                    if !self.newline() {
                        self.advance_char_checked()?;
                    }
                }
                _ => {
                    if !self.newline() {
                        self.advance_char_checked()?;
                    }
                }
            }
        }
        let body = &self.src[body_start..self.pos];
        self.pos += 3;
        // Whitespace escapes are resolved before dedenting, the rest after.
        let joined = remove_ws_escapes(body);
        let dedented = dedent(&joined)
            .map_err(|(_, msg)| self.error_at(ErrorKind::InvalidString, body_start, msg))?;
        let out = unescape(&dedented)
            .map_err(|(_, msg)| self.error_at(ErrorKind::InvalidEscape, body_start, msg))?;
        Ok(Cow::Owned(out))
    }

    fn raw_string(&mut self) -> PResult<Cow<'a, str>> {
        let start = self.pos;
        let mut hashes = 0;
        while self.peek() == Some(b'#') {
            hashes += 1;
            self.pos += 1;
        }
        // The closing delimiter: `quotes` '"' followed by `hashes` '#'.
        let is_close = |p: &Self, quotes: usize| {
            (0..quotes).all(|i| p.peek_at(i) == Some(b'"'))
                && (0..hashes).all(|h| p.peek_at(quotes + h) == Some(b'#'))
        };
        let unterminated =
            |p: &Self| p.error_at(ErrorKind::Unterminated, start, "unterminated raw string");
        if self.starts_with(b"\"\"\"") {
            self.pos += 3;
            if !self.newline() {
                return self.err(
                    ErrorKind::InvalidString,
                    "a multi-line raw string needs a newline after \"\"\"",
                );
            }
            let body_start = self.pos;
            loop {
                if self.eof() {
                    return Err(unterminated(self));
                }
                if is_close(self, 3) {
                    break;
                }
                if !self.newline() {
                    self.advance_char_checked()?;
                }
            }
            let body = &self.src[body_start..self.pos];
            self.pos += 3 + hashes;
            let dedented = dedent(body).map_err(|(off, msg)| {
                self.error_at(ErrorKind::InvalidString, body_start + off, msg)
            })?;
            return Ok(Cow::Owned(dedented));
        }
        self.pos += 1; // opening quote
        let body_start = self.pos;
        loop {
            if self.eof() {
                return Err(unterminated(self));
            }
            if is_close(self, 1) {
                break;
            }
            if self.at_newline() {
                return self.err(
                    ErrorKind::InvalidString,
                    "newline in a single-line raw string",
                );
            }
            self.advance_char_checked()?;
        }
        let body = &self.src[body_start..self.pos];
        self.pos += 1 + hashes;
        Ok(Cow::Borrowed(body))
    }
}

/// Advances `pos` over whole 8-byte chunks made only of plain ASCII
/// (`0x20..=0x7E` other than `"` and `\`), testing eight bytes at once.
/// Stops at the first chunk that holds anything else; the caller's byte
/// loop handles that chunk exactly.
#[inline]
fn skip_plain_ascii(b: &[u8], mut pos: usize) -> usize {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    // Non-zero iff some byte of `v` is zero.
    #[inline(always)]
    fn has_zero(v: u64) -> u64 {
        v.wrapping_sub(LO) & !v & HI
    }
    while let Some(chunk) = b.get(pos..pos + 8) {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(chunk);
        let x = u64::from_le_bytes(bytes);
        let special = has_zero(x ^ (LO * b'"' as u64))
            | has_zero(x ^ (LO * b'\\' as u64))
            | has_zero(x ^ (LO * 0x7F))
            // some byte < 0x20
            | (x.wrapping_sub(LO * 0x20) & !x & HI)
            // some byte >= 0x80
            | (x & HI);
        if special != 0 {
            break;
        }
        pos += 8;
    }
    pos
}

fn strip_underscores(s: &str) -> Cow<'_, str> {
    if s.contains('_') {
        Cow::Owned(s.chars().filter(|&c| c != '_').collect())
    } else {
        Cow::Borrowed(s)
    }
}

fn to_i128(magnitude: u128, negative: bool) -> Option<i128> {
    if negative {
        if magnitude == 1u128 << 127 {
            Some(i128::MIN)
        } else {
            i128::try_from(magnitude).ok().map(|v| -v)
        }
    } else {
        i128::try_from(magnitude).ok()
    }
}

/// Splits `s` into lines on any KDL newline, as `(offset, line)`.
fn split_lines(s: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut iter = s.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if is_newline_char(c) {
            lines.push((line_start, &s[line_start..i]));
            let mut end = i + c.len_utf8();
            if c == '\r' && iter.peek().map(|&(_, n)| n) == Some('\n') {
                iter.next();
                end += 1;
            }
            line_start = end;
        }
    }
    lines.push((line_start, &s[line_start..]));
    lines
}

/// Dedents a multi-line string body. `body` runs from just after the
/// opening newline up to (not including) the closing `"""`; its last line
/// is the indentation prefix. Escapes are not interpreted here; callers
/// resolve whitespace escapes before and the others after.
fn dedent(body: &str) -> Result<String, (usize, &'static str)> {
    let lines = split_lines(body);
    let Some((&(last_off, prefix), content)) = lines.split_last() else {
        return Ok(String::new());
    };
    if !prefix.chars().all(is_unicode_space) {
        return Err((
            last_off,
            "the closing \"\"\" of a multi-line string must be on its own line",
        ));
    }
    let mut out = String::with_capacity(body.len());
    for (i, &(off, line)) in content.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.chars().all(is_unicode_space) {
            continue;
        }
        match line.strip_prefix(prefix) {
            Some(rest) => out.push_str(rest),
            None => {
                return Err((
                    off,
                    "a multi-line string line does not start with the closing line's indentation",
                ));
            }
        }
    }
    Ok(out)
}

/// Removes whitespace escapes (`\` followed by whitespace or newlines),
/// leaving every other escape untouched.
fn remove_ws_escapes(body: &str) -> Cow<'_, str> {
    if !body.contains('\\') {
        return Cow::Borrowed(body);
    }
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some(&n) if is_unicode_space(n) || is_newline_char(n) => {
                while chars
                    .peek()
                    .is_some_and(|&c| is_unicode_space(c) || is_newline_char(c))
                {
                    chars.next();
                }
            }
            Some(&n) => {
                out.push('\\');
                out.push(n);
                chars.next();
            }
            None => out.push('\\'),
        }
    }
    Cow::Owned(out)
}

/// Resolves the escapes in a quoted string body.
fn unescape(body: &str) -> Result<String, (usize, String)> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some((_, e)) = chars.next() else {
            return Err((i, "dangling backslash".into()));
        };
        match e {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'b' => out.push('\u{08}'),
            'f' => out.push('\u{0C}'),
            's' => out.push(' '),
            'u' => {
                if chars.next().map(|(_, c)| c) != Some('{') {
                    return Err((i, "expected `{` after `\\u`".into()));
                }
                let mut hex = String::new();
                loop {
                    match chars.next() {
                        Some((_, '}')) => break,
                        Some((_, h)) if h.is_ascii_hexdigit() && hex.len() < 6 => hex.push(h),
                        _ => return Err((i, "invalid unicode escape".into())),
                    }
                }
                let ch = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| (i, "invalid unicode scalar value in escape".to_owned()))?;
                out.push(ch);
            }
            c if is_unicode_space(c) || is_newline_char(c) => {
                while chars
                    .peek()
                    .is_some_and(|&(_, c)| is_unicode_space(c) || is_newline_char(c))
                {
                    chars.next();
                }
            }
            other => return Err((i, format!("invalid escape `\\{other}`"))),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_plain_ascii_matches_bytewise_scan() {
        fn plain(b: u8) -> bool {
            (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\'
        }
        let specials = [b'"', b'\\', 0x7F, 0x1F, 0x00, b'\n', b'\t', 0x80, 0xE3];
        let base = b"abcdefghijklmnopqrstuvwxyz0123456789-./:=ABCDEFGH";
        for len in 0..base.len() {
            for &sp in &specials {
                for at in 0..=len {
                    let mut v = base[..len].to_vec();
                    v.insert(at, sp);
                    for start in 0..=2.min(v.len()) {
                        let skipped = skip_plain_ascii(&v, start);
                        assert!(v[start..skipped].iter().all(|&b| plain(b)));
                        assert_eq!((skipped - start) % 8, 0);
                        let rest = &v[skipped..];
                        if rest.len() >= 8 {
                            assert!(!rest[..8].iter().all(|&b| plain(b)));
                        }
                    }
                }
            }
        }
    }
}
