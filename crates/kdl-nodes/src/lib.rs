//! Untyped helpers for the most common KDL shape: named blocks,
//! `<kind> <name> { body }`.
//!
//! ```
//! let loaded = kdl_load::load_str("schema.kdl", r#"
//!     struct User { field id "u64" }
//!     struct Post { field title "string" }
//! "#).unwrap();
//! let structs = kdl_nodes::named_blocks(loaded.tree(), "struct", loaded.sources()).unwrap();
//! let names: Vec<&str> = structs.iter().map(|b| b.name).collect();
//! assert_eq!(names, ["User", "Post"]);
//! assert_eq!(kdl_nodes::children(structs[0].node).count(), 1);
//! ```
//!
//! Every function takes a [`kdl_tree::Document`] (from `kdl-load`'s
//! `Loaded::tree`) and its nodes, through the [`Doc`] and [`NodeLike`]
//! traits.
//!
//! Errors carry stable codes (`kdl_nodes::duplicate_name`,
//! `kdl_nodes::expected_identifier`) and [`Label`]s resolved through the
//! document's [`SourceMap`]; with the `miette` feature they are also
//! `miette::Diagnostic`s.

#![no_std]

extern crate alloc;

#[cfg(feature = "miette")]
use alloc::boxed::Box;
use alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};

use alloc::sync::Arc;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use kdl_span::{Label, SourceFile, SourceMap, Span, Spanned, SpannedNode};
use kdl_tree::{Document, Node, Value};

/// A list of nodes: a document or a children block.
pub trait Doc {
    /// Its node type.
    type Node: NodeLike<Doc = Self>;

    /// The nodes, in order.
    fn doc_nodes(&self) -> &[Self::Node];
}

/// A node, as kdl-nodes reads it.
pub trait NodeLike: SpannedNode {
    /// Its children block type.
    type Doc: Doc<Node = Self>;
    /// Its argument type.
    type Arg: Spanned;

    /// The node's name.
    fn node_name(&self) -> &str;

    /// Its children block.
    fn node_children(&self) -> Option<&Self::Doc>;

    /// Its positional arguments, in order.
    fn node_args(&self) -> impl Iterator<Item = &Self::Arg>;

    /// An argument's string value (`None` for other values).
    fn arg_str(arg: &Self::Arg) -> Option<&str>;
}

impl<'s> Doc for Document<'s> {
    type Node = Node<'s>;

    fn doc_nodes(&self) -> &[Node<'s>] {
        self.nodes()
    }
}

impl<'s> NodeLike for Node<'s> {
    type Doc = Document<'s>;
    type Arg = kdl_tree::Entry<'s>;

    fn node_name(&self) -> &str {
        self.name().value()
    }

    fn node_children(&self) -> Option<&Document<'s>> {
        self.children()
    }

    fn node_args(&self) -> impl Iterator<Item = &Self::Arg> {
        self.args()
    }

    fn arg_str(arg: &Self::Arg) -> Option<&str> {
        match arg.value() {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// A node with its name argument: `struct User { … }` has kind `struct` and
/// name `User`.
#[derive(Debug)]
pub struct Named<'a, N = Node<'a>> {
    /// The whole node.
    pub node: &'a N,
    /// Its first argument.
    pub name: &'a str,
}

impl<N> Clone for Named<'_, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<N> Copy for Named<'_, N> {}

impl<'a, N: NodeLike> Named<'a, N> {
    /// The node's children block, if it has one.
    pub fn body(&self) -> Option<&'a N::Doc> {
        self.node.node_children()
    }
}

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Two named nodes of one kind share a name.
    DuplicateName,
    /// A node does not have exactly one string argument as its name.
    ExpectedIdentifier,
}

/// An error about a node's shape.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    kind: ErrorKind,
    message: String,
    help: Option<String>,
    labels: Vec<Label>,
    file: Option<Arc<SourceFile>>,
}

impl Error {
    fn new(kind: ErrorKind, message: String, sources: &SourceMap, labels: Vec<Label>) -> Self {
        let file = labels
            .first()
            .map(|l| Arc::clone(sources.file(l.span.file)));
        Self {
            kind,
            message,
            help: None,
            labels,
            file,
        }
    }

    /// What kind of error this is.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::DuplicateName => "kdl_nodes::duplicate_name",
            ErrorKind::ExpectedIdentifier => "kdl_nodes::expected_identifier",
        }
    }

    /// A suggested fix.
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// Labeled spans, primary first.
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    /// The file the primary label points into.
    pub fn source_file(&self) -> Option<&Arc<SourceFile>> {
        self.file.as_ref()
    }
}

#[cfg(feature = "miette")]
impl miette::Diagnostic for Error {
    fn code<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new(Error::code(self)))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        Some(Box::new(format!(
            "https://github.com/talaagent/kdl-kit/blob/main/docs/errors.md#{}",
            Error::code(self).replace("::", "-")
        )))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn core::fmt::Display + 'a>> {
        self.help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn core::fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.file.as_deref().map(|f| f as &dyn miette::SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        if self.labels.is_empty() {
            return None;
        }
        Some(Box::new(self.labels.iter().map(Label::to_miette)))
    }
}

/// The child nodes of `node` (none if it has no children block).
pub fn children<N: NodeLike>(node: &N) -> impl Iterator<Item = &N> {
    node.node_children().into_iter().flat_map(Doc::doc_nodes)
}

/// The nodes of `doc` named `kind`.
pub fn nodes<'a, D: Doc>(doc: &'a D, kind: &'a str) -> impl Iterator<Item = &'a D::Node> {
    doc.doc_nodes()
        .iter()
        .filter(move |n| n.node_name() == kind)
}

/// The first child of `node` named `name`.
pub fn child<'a, N: NodeLike>(node: &'a N, name: &str) -> Option<&'a N> {
    children(node).find(|c| c.node_name() == name)
}

/// The node's name: its single positional string argument
/// (`struct User { … }` → `User`). Properties and children are allowed.
pub fn identifier<'a, N: NodeLike>(node: &'a N, sources: &SourceMap) -> Result<&'a str, Error> {
    let mut args = node.node_args();
    let kind = node.node_name();
    let fail = |message: String, at: Option<Span>, label: &str| {
        let labels = at
            .or_else(|| sources.node_name(node))
            .map(|s| vec![Label::primary(s, label)])
            .unwrap_or_default();
        let mut e = Error::new(ErrorKind::ExpectedIdentifier, message, sources, labels);
        e.help = Some(format!("write it as `{kind} <name> {{ … }}`"));
        e
    };
    let Some(first) = args.next() else {
        return Err(fail(format!("`{kind}` needs a name"), None, "missing name"));
    };
    let Some(name) = N::arg_str(first) else {
        return Err(fail(
            format!("the name of `{kind}` must be a string"),
            sources.entry(first),
            "not a string",
        ));
    };
    if let Some(extra) = args.next() {
        return Err(fail(
            format!("`{kind}` takes a single name"),
            sources.entry(extra),
            "unexpected argument",
        ));
    }
    Ok(name)
}

/// The `kind` nodes of `doc` with their names, in document order. Fails
/// on a node without a single name argument, or on a name used twice.
pub fn named_blocks<'a, D: Doc>(
    doc: &'a D,
    kind: &str,
    sources: &SourceMap,
) -> Result<Vec<Named<'a, D::Node>>, Error> {
    let blocks = doc
        .doc_nodes()
        .iter()
        .filter(|n| n.node_name() == kind)
        .map(|node| {
            Ok(Named {
                node,
                name: identifier(node, sources)?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    check_unique(&blocks, sources)?;
    Ok(blocks)
}

/// Fails if two of `blocks` share a name, pointing at both.
pub fn check_unique<N: NodeLike>(
    blocks: &[Named<'_, N>],
    sources: &SourceMap,
) -> Result<(), Error> {
    let mut seen: alloc::collections::BTreeMap<&str, &Named<'_, N>> =
        alloc::collections::BTreeMap::new();
    for block in blocks {
        if let Some(first) = seen.insert(block.name, block) {
            let kind = block.node.node_name();
            let arg = |n: &N| n.node_args().next().and_then(|e| sources.entry(e));
            let mut labels = Vec::new();
            if let Some(s) = arg(block.node) {
                labels.push(Label::primary(s, "defined again here"));
            }
            let mut help = None;
            if let (Some(s), Some(again)) = (arg(first.node), labels.first()) {
                if s.file == again.span.file {
                    labels.push(Label::secondary(s, "first defined here"));
                } else {
                    help = Some(format!("first defined at {}", sources.describe(s)));
                }
            }
            let mut e = Error::new(
                ErrorKind::DuplicateName,
                format!("duplicate {kind} `{}`", block.name),
                sources,
                labels,
            );
            e.help = help.or_else(|| Some("rename or remove one of them".to_owned()));
            return Err(e);
        }
    }
    Ok(())
}

/// Every node in `doc`, depth first, with its ancestors (outermost first).
pub fn walk<'a, D: Doc>(doc: &'a D, mut f: impl FnMut(&[&'a D::Node], &'a D::Node)) {
    fn go<'a, N: NodeLike>(
        nodes: &'a [N],
        path: &mut Vec<&'a N>,
        f: &mut impl FnMut(&[&'a N], &'a N),
    ) {
        for node in nodes {
            f(path, node);
            if let Some(children) = node.node_children() {
                path.push(node);
                go(children.doc_nodes(), path, f);
                path.pop();
            }
        }
    }
    go(doc.doc_nodes(), &mut Vec::new(), &mut f);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn load(s: &str) -> (Document<'_>, SourceMap) {
        let mut sources = SourceMap::new();
        sources.add("t.kdl", s);
        (Document::parse(s).unwrap(), sources)
    }

    #[test]
    fn named_and_duplicates() {
        let (doc, sm) = load("struct A { x }\nenum B\nstruct C\nstruct A");
        let e = named_blocks(&doc, "struct", &sm).unwrap_err();
        assert_eq!(e.code(), "kdl_nodes::duplicate_name");
        assert_eq!(e.to_string(), "duplicate struct `A`");
        assert_eq!(e.labels().len(), 2);
        assert_eq!(
            sm.file(e.labels()[0].span.file)
                .line_col(e.labels()[0].span.offset),
            (4, 8)
        );
    }

    #[test]
    fn identifiers() {
        let (doc, sm) = load("a\nb 1\nc x y\nd x k=1 { }");
        let ids: Vec<_> = doc
            .nodes()
            .iter()
            .map(|n| identifier(n, &sm).map_err(|e| e.to_string()))
            .collect();
        assert_eq!(
            ids,
            [
                Err("`a` needs a name".into()),
                Err("the name of `b` must be a string".into()),
                Err("`c` takes a single name".into()),
                Ok("x"),
            ]
        );
    }

    #[test]
    fn walks_depth_first() {
        let (doc, _) = load("a { b { c }; d }\ne");
        let mut seen = Vec::new();
        walk(&doc, |path, n| {
            seen.push(format!("{}{}", path.len(), n.name().value()));
        });
        assert_eq!(seen, ["0a", "1b", "2c", "1d", "0e"]);
    }
}
