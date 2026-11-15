//! A fast KDL v2 parser into a small document tree with a byte span on
//! everything: node, name, type annotation, entry, property key, value and
//! children block.
//!
//! The tree borrows from the text it was parsed from (strings that need no
//! unescaping are `Cow::Borrowed`); [`Document::into_owned`] detaches it.
//! Comments, whitespace and the original spelling of values are not kept:
//! the tree is for reading. Use the `kdl` crate to edit or format KDL.
//!
//! Spans are byte offsets plus [`ParseOptions::base`], so a parser that
//! reads several files into one offset space (see `kdl-span`'s
//! `SourceMap`) gets global offsets without a second pass.
//!
//! ```
//! let doc = kdl_tree::Document::parse(r#"(t)server "api" port=8080 { tls #true }"#).unwrap();
//! let server = &doc.nodes()[0];
//! assert_eq!(server.name().value(), "server");
//! assert_eq!(server.ty().map(|t| t.value()), Some("t"));
//! let port = &server.entries()[1];
//! assert_eq!(port.name().map(|k| k.value()), Some("port"));
//! assert_eq!(port.value(), &kdl_tree::Value::Integer(8080));
//! assert_eq!((port.span().offset, port.span().len), (16, 9));
//! assert_eq!((port.value_span().offset, port.value_span().len), (21, 4));
//! let children = server.children().unwrap();
//! assert_eq!(children.nodes()[0].name().value(), "tls");
//! ```
//!
//! Nesting is capped ([`ParseOptions::max_depth`], 256 by default): deeper
//! input is an [`ErrorKind::TooDeep`] error, never a stack overflow.
//!
//! The parser started as a copy of club-kdl's single-pass parser
//! (MIT OR Apache-2.0); see `src/parse.rs` and the workspace NOTICE.

extern crate alloc;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(feature = "kdl")]
mod compat;
mod error;
mod parse;

use alloc::borrow::Cow;

pub use crate::error::{Error, ErrorKind};

/// A byte range: `offset` (plus the parse's [`ParseOptions::base`]) and
/// length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Span {
    /// The first byte.
    pub offset: usize,
    /// The length in bytes.
    pub len: usize,
}

impl Span {
    /// A span of `len` bytes at `offset`.
    pub const fn new(offset: usize, len: usize) -> Self {
        Self { offset, len }
    }

    /// One past the last byte.
    pub const fn end(self) -> usize {
        self.offset + self.len
    }
}

/// How to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseOptions {
    /// Added to every offset in the tree and in errors.
    pub base: usize,
    /// The deepest children-block nesting accepted.
    pub max_depth: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            base: 0,
            max_depth: 256,
        }
    }
}

impl ParseOptions {
    /// Offsets start at `base` instead of 0.
    #[must_use]
    pub fn base(mut self, base: usize) -> Self {
        self.base = base;
        self
    }

    /// Accept at most `depth` nested children blocks.
    #[must_use]
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

/// A document, or a node's children block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document<'a> {
    nodes: Vec<Node<'a>>,
    span: Span,
}

impl<'a> Document<'a> {
    /// Parses a KDL v2 document with the default [`ParseOptions`].
    pub fn parse(src: &'a str) -> Result<Self, Error> {
        Self::parse_with(src, &ParseOptions::default())
    }

    /// Parses a KDL v2 document.
    pub fn parse_with(src: &'a str, opts: &ParseOptions) -> Result<Self, Error> {
        parse::parse(src, opts)
    }

    /// A document with these nodes and span.
    pub fn new(nodes: Vec<Node<'a>>, span: Span) -> Self {
        Self { nodes, span }
    }

    /// The nodes, in order (slashdashed ones are gone).
    pub fn nodes(&self) -> &[Node<'a>] {
        &self.nodes
    }

    /// The nodes, mutably (to merge documents).
    pub fn nodes_mut(&mut self) -> &mut Vec<Node<'a>> {
        &mut self.nodes
    }

    /// The whole input for a document; the braces and everything between
    /// them for a children block.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The same tree, owning all its strings.
    pub fn into_owned(self) -> Document<'static> {
        Document {
            nodes: self.nodes.into_iter().map(Node::into_owned).collect(),
            span: self.span,
        }
    }
}

/// A node: `(type)name args… props… { children }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Node<'a> {
    ty: Option<Ident<'a>>,
    name: Ident<'a>,
    entries: Vec<Entry<'a>>,
    children: Option<Document<'a>>,
    span: Span,
}

impl<'a> Node<'a> {
    /// A node from its parts.
    pub fn new(
        ty: Option<Ident<'a>>,
        name: Ident<'a>,
        entries: Vec<Entry<'a>>,
        children: Option<Document<'a>>,
        span: Span,
    ) -> Self {
        Self {
            ty,
            name,
            entries,
            children,
            span,
        }
    }

    /// The type annotation.
    pub fn ty(&self) -> Option<&Ident<'a>> {
        self.ty.as_ref()
    }

    /// The name.
    pub fn name(&self) -> &Ident<'a> {
        &self.name
    }

    /// Arguments and properties in written order (duplicate property keys
    /// included; the last one is the property's value).
    pub fn entries(&self) -> &[Entry<'a>] {
        &self.entries
    }

    /// The positional arguments.
    pub fn args(&self) -> impl Iterator<Item = &Entry<'a>> {
        self.entries.iter().filter(|e| e.name.is_none())
    }

    /// The properties, in written order.
    pub fn props(&self) -> impl Iterator<Item = &Entry<'a>> {
        self.entries.iter().filter(|e| e.name.is_some())
    }

    /// The children block, if there is one (it may be empty).
    pub fn children(&self) -> Option<&Document<'a>> {
        self.children.as_ref()
    }

    /// From the type annotation (or name) to the end of the last entry or
    /// the closing brace.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The same node, owning all its strings.
    pub fn into_owned(self) -> Node<'static> {
        Node {
            ty: self.ty.map(Ident::into_owned),
            name: self.name.into_owned(),
            entries: self.entries.into_iter().map(Entry::into_owned).collect(),
            children: self.children.map(Document::into_owned),
            span: self.span,
        }
    }
}

/// A node name, type annotation or property key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident<'a> {
    value: Cow<'a, str>,
    span: Span,
}

impl<'a> Ident<'a> {
    /// An identifier from its value and span.
    pub fn new(value: impl Into<Cow<'a, str>>, span: Span) -> Self {
        Self {
            value: value.into(),
            span,
        }
    }

    /// The (unescaped) text.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The text, borrowed from the source when it needed no unescaping.
    pub fn as_cow(&self) -> &Cow<'a, str> {
        &self.value
    }

    /// The string as written, quotes included; for a type annotation, the
    /// string inside the parentheses.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The same identifier, owning its string.
    pub fn into_owned(self) -> Ident<'static> {
        Ident {
            value: Cow::Owned(self.value.into_owned()),
            span: self.span,
        }
    }
}

/// An argument (`(type)value`) or a property (`key=(type)value`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    name: Option<Ident<'a>>,
    ty: Option<Ident<'a>>,
    value: Value<'a>,
    value_span: Span,
    span: Span,
}

impl<'a> Entry<'a> {
    /// An entry from its parts.
    pub fn new(
        name: Option<Ident<'a>>,
        ty: Option<Ident<'a>>,
        value: Value<'a>,
        value_span: Span,
        span: Span,
    ) -> Self {
        Self {
            name,
            ty,
            value,
            value_span,
            span,
        }
    }

    /// The property key; `None` for an argument.
    pub fn name(&self) -> Option<&Ident<'a>> {
        self.name.as_ref()
    }

    /// The value's type annotation.
    pub fn ty(&self) -> Option<&Ident<'a>> {
        self.ty.as_ref()
    }

    /// The value.
    pub fn value(&self) -> &Value<'a> {
        &self.value
    }

    /// The value as written (without its type annotation).
    pub fn value_span(&self) -> Span {
        self.value_span
    }

    /// The whole entry, from the key or type annotation to the end of the
    /// value.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The same entry, owning its strings.
    pub fn into_owned(self) -> Entry<'static> {
        Entry {
            name: self.name.map(Ident::into_owned),
            ty: self.ty.map(Ident::into_owned),
            value: self.value.into_owned(),
            value_span: self.value_span,
            span: self.span,
        }
    }
}

/// A KDL value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// Any string form: identifier, quoted, raw or multi-line.
    String(Cow<'a, str>),
    /// An integer (decimal, hex, octal or binary).
    Integer(i128),
    /// A float, including `#inf`, `#-inf` and `#nan`.
    Float(f64),
    /// `#true` or `#false`.
    Bool(bool),
    /// `#null`.
    Null,
}

impl Value<'_> {
    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// The same value, owning its string.
    pub fn into_owned(self) -> Value<'static> {
        match self {
            Self::String(s) => Value::String(Cow::Owned(s.into_owned())),
            Self::Integer(i) => Value::Integer(i),
            Self::Float(f) => Value::Float(f),
            Self::Bool(b) => Value::Bool(b),
            Self::Null => Value::Null,
        }
    }
}
