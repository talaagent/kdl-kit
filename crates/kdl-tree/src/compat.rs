//! Conversions to and from `kdl`'s document tree (the `kdl` feature).

use alloc::borrow::Cow;

use kdl::{KdlDocument, KdlEntry, KdlIdentifier, KdlNode, KdlValue};

use crate::{Document, Entry, Ident, Node, Span, Value};

/// The span of a `kdl` item (its `SourceSpan` type is miette's).
macro_rules! span {
    ($e:expr) => {{
        let s = $e;
        Span::new(s.offset(), s.len())
    }};
}

fn kdl_span(s: Span) -> (usize, usize) {
    (s.offset, s.len)
}

impl<'a> Document<'a> {
    /// A view of a `kdl` document, borrowing its strings. Spans are
    /// `kdl`'s: an entry's value span is the whole entry, a children
    /// block's span is its contents without the braces.
    pub fn from_kdl(doc: &'a KdlDocument) -> Self {
        Self::new(
            doc.nodes().iter().map(Node::from_kdl).collect(),
            span!(doc.span()),
        )
    }

    /// The same tree as a `kdl` document, with the spans set (`kdl`
    /// formatting defaults; comments and spelling are not recovered).
    pub fn to_kdl(&self) -> KdlDocument {
        let mut doc = KdlDocument::new();
        doc.nodes_mut()
            .extend(self.nodes().iter().map(Node::to_kdl));
        doc.set_span(kdl_span(self.span()));
        doc
    }
}

impl<'a> Node<'a> {
    /// A view of a `kdl` node; see [`Document::from_kdl`].
    pub fn from_kdl(node: &'a KdlNode) -> Self {
        Self::new(
            node.ty().map(Ident::from_kdl),
            Ident::from_kdl(node.name()),
            node.entries().iter().map(Entry::from_kdl).collect(),
            node.children().map(Document::from_kdl),
            span!(node.span()),
        )
    }

    /// The same node as a `kdl` node; see [`Document::to_kdl`].
    pub fn to_kdl(&self) -> KdlNode {
        let mut node = KdlNode::new(self.name().to_kdl());
        if let Some(ty) = self.ty() {
            node.set_ty(ty.to_kdl());
        }
        node.entries_mut()
            .extend(self.entries().iter().map(Entry::to_kdl));
        if let Some(children) = self.children() {
            node.set_children(children.to_kdl());
        }
        node.set_span(kdl_span(self.span()));
        node
    }
}

impl<'a> Ident<'a> {
    fn from_kdl(ident: &'a KdlIdentifier) -> Self {
        Self::new(ident.value(), span!(ident.span()))
    }

    fn to_kdl(&self) -> KdlIdentifier {
        let mut ident = KdlIdentifier::from(self.value());
        ident.set_span(kdl_span(self.span()));
        ident
    }
}

impl<'a> Entry<'a> {
    fn from_kdl(entry: &'a KdlEntry) -> Self {
        let value = match entry.value() {
            KdlValue::String(s) => Value::String(Cow::Borrowed(s)),
            KdlValue::Integer(i) => Value::Integer(*i),
            KdlValue::Float(f) => Value::Float(*f),
            KdlValue::Bool(b) => Value::Bool(*b),
            KdlValue::Null => Value::Null,
        };
        Self::new(
            entry.name().map(Ident::from_kdl),
            entry.ty().map(Ident::from_kdl),
            value,
            span!(entry.span()),
            span!(entry.span()),
        )
    }

    fn to_kdl(&self) -> KdlEntry {
        let value = match self.value() {
            Value::String(s) => KdlValue::String(s.as_ref().to_owned()),
            Value::Integer(i) => KdlValue::Integer(*i),
            Value::Float(f) => KdlValue::Float(*f),
            Value::Bool(b) => KdlValue::Bool(*b),
            Value::Null => KdlValue::Null,
        };
        let mut entry = match self.name() {
            Some(key) => KdlEntry::new_prop(key.to_kdl(), value),
            None => KdlEntry::new(value),
        };
        if let Some(ty) = self.ty() {
            entry.set_ty(ty.to_kdl());
        }
        entry.set_span(kdl_span(self.span()));
        entry
    }
}
