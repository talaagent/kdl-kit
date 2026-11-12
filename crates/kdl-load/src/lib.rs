//! Load a KDL string, file or directory of `*.kdl` files as one document,
//! with a [`SourceMap`] so every span in it resolves to a file, line and
//! column.
//!
//! The nodes of all files are concatenated in path order into a single
//! [`kdl_tree::Document`] (re-exported here as [`Document`]). Each file gets
//! its own range in the source map's global offset space and is parsed
//! straight into that range, so every span in the tree (`node.span()`,
//! `entry.span()`, `name().span()`) is global: [`SourceMap::resolve`],
//! [`SourceMap::node_name`] and friends turn them into file-local
//! [`Span`]s.
//!
//! The default `kdl-compat` feature keeps the `kdl::KdlDocument` view
//! ([`Loaded::doc`], [`Loaded::into_parts`]) for one release; it is
//! deprecated and converts the tree on first use.
//!
//! With the default `parallel` feature, files are read and parsed on
//! rayon's thread pool; the merge order is always the sorted path order.
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let dir = tempfile::tempdir()?;
//! # std::fs::write(dir.path().join("a.kdl"), "member ada")?;
//! # std::fs::write(dir.path().join("b.kdl"), "\nmember grace")?;
//! let loaded = kdl_load::load_dir(dir.path(), false)?;
//! let grace = &loaded.tree().nodes()[1];
//! let span = loaded.sources().node_name(grace).unwrap();
//! let file = loaded.sources().file(span.file);
//! assert!(file.name().ends_with("b.kdl"));
//! assert_eq!(file.line_col(span.offset), (2, 1));
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use kdl_span::{FileId, Label, SourceFile, SourceMap, Span, Spanned, SpannedNode};
pub use kdl_tree::{Document, Entry, Ident, Node, Value};

/// A document and the files it came from.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    tree: Document<'static>,
    sources: SourceMap,
    #[cfg(feature = "kdl-compat")]
    kdl: std::sync::OnceLock<kdl::KdlDocument>,
}

impl Loaded {
    fn new(tree: Document<'static>, sources: SourceMap) -> Self {
        Self {
            tree,
            sources,
            #[cfg(feature = "kdl-compat")]
            kdl: std::sync::OnceLock::new(),
        }
    }

    /// The merged document.
    pub fn tree(&self) -> &Document<'static> {
        &self.tree
    }

    /// The source map its spans resolve against.
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// The document and source map.
    pub fn into_tree(self) -> (Document<'static>, SourceMap) {
        (self.tree, self.sources)
    }

    /// The merged document as a `kdl::KdlDocument`, converted from
    /// [`tree`](Self::tree) on first use (spans kept, formatting and
    /// comments not).
    #[cfg(feature = "kdl-compat")]
    #[deprecated(note = "use `tree()`; the `kdl` view goes away with the `kdl-compat` feature")]
    pub fn doc(&self) -> &kdl::KdlDocument {
        self.kdl.get_or_init(|| self.tree.to_kdl())
    }

    /// The document as a `kdl::KdlDocument`, and the source map.
    #[cfg(feature = "kdl-compat")]
    #[deprecated(
        note = "use `into_tree()`; the `kdl` view goes away with the `kdl-compat` feature"
    )]
    pub fn into_parts(self) -> (kdl::KdlDocument, SourceMap) {
        let doc = match self.kdl.into_inner() {
            Some(doc) => doc,
            None => self.tree.to_kdl(),
        };
        (doc, self.sources)
    }
}

/// A file could not be read or parsed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Reading a file or listing a directory failed.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// A file is not valid KDL v2.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

impl Error {
    /// The stable error code: `kdl_load::io` or `kdl_load::parse`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "kdl_load::io",
            Self::Parse(_) => "kdl_load::parse",
        }
    }
}

/// A KDL syntax error, with every problem the parser found in the file.
#[derive(Debug, thiserror::Error)]
#[error("{}", self.first().message)]
pub struct ParseError {
    file: Arc<SourceFile>,
    problems: Vec<ParseProblem>,
}

/// One problem the parser reported.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ParseProblem {
    /// What kind of syntax error; its `code()` is a stable
    /// `kdl_tree::…` code.
    pub kind: kdl_tree::ErrorKind,
    /// What is wrong.
    pub message: String,
    /// Where, with a short label.
    pub label: Label,
    /// How to fix it, when the parser knows.
    pub help: Option<String>,
}

impl ParseError {
    fn new(file: Arc<SourceFile>, err: &kdl_tree::Error) -> Self {
        use kdl_tree::ErrorKind as K;
        let at = err.span();
        let span = Span::new(file.id(), at.offset - file.base(), at.len);
        let (label, help) = match err.kind() {
            K::Unterminated => ("starts here", err.help()),
            K::TooDeep => ("nested too deeply", err.help()),
            K::InvalidString => ("this string", err.help()),
            K::InvalidIdentifier => ("not a value", err.help()),
            K::InvalidKeyword => (
                "unknown keyword",
                Some("the keywords are #true, #false, #null, #inf, #-inf and #nan"),
            ),
            K::ChildrenOrder => ("after the children block", err.help()),
            _ => ("here", err.help()),
        };
        let problem = ParseProblem {
            kind: err.kind(),
            message: err.message().to_owned(),
            label: Label::primary(span, label),
            help: help.map(str::to_owned),
        };
        Self {
            file,
            problems: vec![problem],
        }
    }

    /// What kind of syntax error the first problem is.
    pub fn kind(&self) -> kdl_tree::ErrorKind {
        self.first().kind
    }

    /// The file that failed to parse.
    pub fn file(&self) -> &Arc<SourceFile> {
        &self.file
    }

    /// Every problem found, in source order; never empty.
    pub fn problems(&self) -> &[ParseProblem] {
        &self.problems
    }

    fn first(&self) -> &ParseProblem {
        &self.problems[0]
    }

    /// The stable error code, `kdl_load::parse`.
    pub fn code(&self) -> &'static str {
        "kdl_load::parse"
    }
}

#[cfg(feature = "miette")]
mod diagnostic {
    use super::{Error, ParseError};
    use core::fmt::Display;
    use miette::{Diagnostic, LabeledSpan, SourceCode};

    const DOCS: &str = "https://github.com/talaagent/kdl-serde/blob/main/docs/errors.md";

    impl Diagnostic for Error {
        fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            Some(Box::new(Error::code(self)))
        }

        fn url<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            Some(Box::new(format!(
                "{DOCS}#{}",
                Error::code(self).replace("::", "-")
            )))
        }

        fn source_code(&self) -> Option<&dyn SourceCode> {
            match self {
                Self::Parse(e) => e.source_code(),
                Self::Io { .. } => None,
            }
        }

        fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
            match self {
                Self::Parse(e) => e.labels(),
                Self::Io { .. } => None,
            }
        }

        fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            match self {
                Self::Parse(e) => e.help(),
                Self::Io { .. } => None,
            }
        }

        fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
            match self {
                Self::Parse(e) => e.related(),
                Self::Io { .. } => None,
            }
        }
    }

    impl Diagnostic for ParseError {
        fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            Some(Box::new(ParseError::code(self)))
        }

        fn url<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            Some(Box::new(format!("{DOCS}#kdl_load-parse")))
        }

        fn source_code(&self) -> Option<&dyn SourceCode> {
            Some(&*self.file)
        }

        fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
            Some(Box::new(core::iter::once(self.first().label.to_miette())))
        }

        fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            self.first()
                .help
                .as_ref()
                .map(|h| Box::new(h) as Box<dyn Display>)
        }
    }
}

/// Loads KDL text. `name` is used in diagnostics (pass `""` for none).
pub fn load_str(name: &str, text: impl Into<Arc<str>>) -> Result<Loaded, Error> {
    let mut sources = SourceMap::new();
    let id = sources.add(name, text);
    let tree = parse(sources.file(id))?;
    Ok(Loaded::new(tree, sources))
}

/// Loads one file.
pub fn load_file(path: impl AsRef<Path>) -> Result<Loaded, Error> {
    load_files([path.as_ref().to_owned()])
}

/// Loads a file, or a directory as [`load_dir`] does.
pub fn load_path(path: impl AsRef<Path>, recursive: bool) -> Result<Loaded, Error> {
    let path = path.as_ref();
    let meta = std::fs::metadata(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    if meta.is_dir() {
        load_dir(path, recursive)
    } else {
        load_file(path)
    }
}

/// Loads every `*.kdl` file in `dir` (and its subdirectories when
/// `recursive`), sorted by path, as one document.
pub fn load_dir(dir: impl AsRef<Path>, recursive: bool) -> Result<Loaded, Error> {
    let mut files = Vec::new();
    list_kdl_files(dir.as_ref(), recursive, &mut files)?;
    files.sort();
    load_files(files)
}

/// The `*.kdl` files in `dir` (recursively or not), sorted by path.
pub fn kdl_files(dir: impl AsRef<Path>, recursive: bool) -> Result<Vec<PathBuf>, Error> {
    let mut files = Vec::new();
    list_kdl_files(dir.as_ref(), recursive, &mut files)?;
    files.sort();
    Ok(files)
}

fn list_kdl_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    for entry in std::fs::read_dir(dir).map_err(io(dir))? {
        let entry = entry.map_err(io(dir))?;
        let path = entry.path();
        let ty = entry.file_type().map_err(io(&path))?;
        if ty.is_dir() {
            if recursive {
                list_kdl_files(&path, recursive, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "kdl") {
            out.push(path);
        }
    }
    Ok(())
}

/// Loads the given files, in the given order, as one document.
pub fn load_files(paths: impl IntoIterator<Item = PathBuf>) -> Result<Loaded, Error> {
    let paths: Vec<PathBuf> = paths.into_iter().collect();
    let texts = map_maybe_parallel(&paths, |path| {
        std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })
    });
    let mut sources = SourceMap::new();
    for (path, text) in paths.iter().zip(texts) {
        sources.add(path.display().to_string(), text?);
    }
    let docs = map_maybe_parallel(sources.files(), parse);
    let end = sources
        .files()
        .last()
        .map_or(0, |f| f.base() + f.text().len());
    let mut merged = Document::new(Vec::new(), kdl_tree::Span::new(0, end));
    for doc in docs {
        merged.nodes_mut().append(doc?.nodes_mut());
    }
    Ok(Loaded::new(merged, sources))
}

#[cfg(feature = "parallel")]
fn map_maybe_parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    use rayon::prelude::*;
    if items.len() < 2 {
        return items.iter().map(f).collect();
    }
    items.par_iter().map(f).collect()
}

#[cfg(not(feature = "parallel"))]
fn map_maybe_parallel<T, R>(items: &[T], f: impl Fn(&T) -> R) -> Vec<R> {
    items.iter().map(f).collect()
}

fn parse(file: &Arc<SourceFile>) -> Result<Document<'static>, Error> {
    let opts = kdl_tree::ParseOptions::default().base(file.base());
    match Document::parse_with(file.text(), &opts) {
        Ok(doc) => Ok(doc.into_owned()),
        Err(e) => Err(ParseError::new(Arc::clone(file), &e).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_resolve_to_their_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.kdl"), "x 1\ny (t)2 k=3 { z }").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.kdl"), "\n\n  w k=(t)1 { v }").unwrap();
        std::fs::write(dir.path().join("c.txt"), "not kdl {").unwrap();

        let flat = load_dir(dir.path(), false).unwrap();
        assert_eq!(flat.tree().nodes().len(), 2);

        let all = load_dir(dir.path(), true).unwrap();
        let sm = all.sources();
        let w = &all.tree().nodes()[2];
        let at = |s: Span| {
            let f = sm.file(s.file);
            let (l, c) = f.line_col(s.offset);
            (f.name().rsplit('/').next().unwrap_or("").to_owned(), l, c)
        };
        assert_eq!(at(sm.node_name(w).unwrap()), ("b.kdl".into(), 3, 3));
        assert_eq!(
            at(sm.entry(&w.entries()[0]).unwrap()),
            ("b.kdl".into(), 3, 5)
        );
        let v = &w.children().unwrap().nodes()[0];
        assert_eq!(at(sm.node_name(v).unwrap()), ("b.kdl".into(), 3, 14));
        let y = &all.tree().nodes()[1];
        assert_eq!(
            at(sm.ident(y.entries()[0].ty().unwrap()).unwrap()),
            ("a.kdl".into(), 2, 4)
        );
    }

    #[test]
    fn parse_errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.kdl"), "ok 1").unwrap();
        std::fs::write(dir.path().join("b.kdl"), "bad {").unwrap();
        let Err(Error::Parse(e)) = load_dir(dir.path(), false) else {
            panic!("expected a parse error")
        };
        assert!(e.file().name().ends_with("b.kdl"));
        assert_eq!(e.code(), "kdl_load::parse");
        assert_eq!(e.kind(), kdl_tree::ErrorKind::Unterminated);
        assert_eq!(e.problems()[0].label.span.offset, 4);
    }

    #[test]
    #[cfg(feature = "kdl-compat")]
    #[allow(deprecated)]
    fn kdl_view_keeps_global_spans() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.kdl"), "x 1").unwrap();
        std::fs::write(dir.path().join("b.kdl"), "\n  y k=2").unwrap();
        let loaded = load_dir(dir.path(), false).unwrap();
        let y = &loaded.doc().nodes()[1];
        assert_eq!(y.name().value(), "y");
        let span = loaded.sources().node_name(y).unwrap();
        assert_eq!(
            loaded.sources().file(span.file).line_col(span.offset),
            (2, 3)
        );
        let k = loaded.sources().entry(&y.entries()[0]).unwrap();
        assert_eq!((k.offset, k.len), (5, 3));
    }

    #[test]
    fn missing_dir_is_io() {
        let e = load_dir("/nonexistent/kdl-load-test", true).unwrap_err();
        assert_eq!(e.code(), "kdl_load::io");
    }
}
