//! Untyped helpers for the most common KDL shape: named blocks,
//! `<kind> <name> { body }`.
//!
//! ```
//! let loaded = kdl_load::load_str("schema.kdl", r#"
//!     struct User { field id "u64" }
//!     struct Post { field title "string" }
//! "#).unwrap();
//! let structs = kdl_nodes::named_blocks(loaded.doc(), "struct", loaded.sources()).unwrap();
//! let names: Vec<&str> = structs.iter().map(|b| b.name).collect();
//! assert_eq!(names, ["User", "Post"]);
//! assert_eq!(kdl_nodes::children(structs[0].node).count(), 1);
//! ```
//!
//! Errors carry stable codes (`kdl_nodes::duplicate_name`,
//! `kdl_nodes::expected_identifier`) and [`Label`]s resolved through the
//! document's [`SourceMap`]; with the `miette` feature they are also
//! `miette::Diagnostic`s.

use std::sync::Arc;

use kdl::{KdlDocument, KdlNode, KdlValue};
pub use kdl_span::{Label, SourceFile, SourceMap, Span};

/// A node with its name argument: `struct User { … }` has kind `struct` and
/// name `User`.
#[derive(Debug, Clone, Copy)]
pub struct Named<'a> {
    /// The whole node.
    pub node: &'a KdlNode,
    /// Its first argument.
    pub name: &'a str,
}

impl<'a> Named<'a> {
    /// The node's children block, if it has one.
    pub fn body(&self) -> Option<&'a KdlDocument> {
        self.node.children()
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
            "https://github.com/talaagent/kdl-serde/blob/main/docs/errors.md#{}",
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
pub fn children(node: &KdlNode) -> impl Iterator<Item = &KdlNode> {
    node.children().into_iter().flat_map(KdlDocument::nodes)
}

/// The nodes of `doc` named `kind`.
pub fn nodes<'a>(doc: &'a KdlDocument, kind: &'a str) -> impl Iterator<Item = &'a KdlNode> {
    doc.nodes().iter().filter(move |n| n.name().value() == kind)
}

/// The first child of `node` named `name`.
pub fn child<'a>(node: &'a KdlNode, name: &str) -> Option<&'a KdlNode> {
    children(node).find(|c| c.name().value() == name)
}

/// The node's name: its single positional string argument
/// (`struct User { … }` → `User`). Properties and children are allowed.
pub fn identifier<'a>(node: &'a KdlNode, sources: &SourceMap) -> Result<&'a str, Error> {
    let mut args = node.entries().iter().filter(|e| e.name().is_none());
    let kind = node.name().value();
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
    let KdlValue::String(name) = first.value() else {
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
pub fn named_blocks<'a>(
    doc: &'a KdlDocument,
    kind: &str,
    sources: &SourceMap,
) -> Result<Vec<Named<'a>>, Error> {
    let blocks = doc
        .nodes()
        .iter()
        .filter(|n| n.name().value() == kind)
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
pub fn check_unique(blocks: &[Named<'_>], sources: &SourceMap) -> Result<(), Error> {
    let mut seen: std::collections::HashMap<&str, &Named<'_>> =
        std::collections::HashMap::with_capacity(blocks.len());
    for block in blocks {
        if let Some(first) = seen.insert(block.name, block) {
            let kind = block.node.name().value();
            let arg = |n: &KdlNode| {
                n.entries()
                    .iter()
                    .find(|e| e.name().is_none())
                    .and_then(|e| sources.entry(e))
            };
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
pub fn walk<'a>(doc: &'a KdlDocument, mut f: impl FnMut(&[&'a KdlNode], &'a KdlNode)) {
    fn go<'a>(
        nodes: &'a [KdlNode],
        path: &mut Vec<&'a KdlNode>,
        f: &mut impl FnMut(&[&'a KdlNode], &'a KdlNode),
    ) {
        for node in nodes {
            f(path, node);
            if let Some(children) = node.children() {
                path.push(node);
                go(children.nodes(), path, f);
                path.pop();
            }
        }
    }
    go(doc.nodes(), &mut Vec::new(), &mut f);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(s: &str) -> (KdlDocument, SourceMap) {
        let mut sources = SourceMap::new();
        sources.add("t.kdl", s);
        let doc: KdlDocument = s.parse().unwrap();
        (doc, sources)
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
            seen.push(format!("{}{}", path.len(), n.name().value()))
        });
        assert_eq!(seen, ["0a", "1b", "2c", "1d", "0e"]);
    }
}
