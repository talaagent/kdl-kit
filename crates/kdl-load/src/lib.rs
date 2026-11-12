//! Load a KDL string, file or directory of `*.kdl` files as one document,
//! with a [`SourceMap`] so every span in it resolves to a file, line and
//! column.
//!
//! The nodes of all files are concatenated in path order into a single
//! [`KdlDocument`]. Each file gets its own range in the source map's global
//! offset space and its spans are shifted into that range, so plain `kdl`
//! spans (`node.span()`, `entry.span()`) stay meaningful:
//! [`SourceMap::node_name`] and friends turn them into file-local
//! [`Span`]s.
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
//! let grace = &loaded.doc().nodes()[1];
//! let span = loaded.sources().node_name(grace).unwrap();
//! let file = loaded.sources().file(span.file);
//! assert!(file.name().ends_with("b.kdl"));
//! assert_eq!(file.line_col(span.offset), (2, 1));
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kdl::{KdlDocument, KdlNode};
pub use kdl_span::{FileId, Label, SourceFile, SourceMap, Span};

/// A document and the files it came from.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    doc: KdlDocument,
    sources: SourceMap,
}

impl Loaded {
    /// The merged document.
    pub fn doc(&self) -> &KdlDocument {
        &self.doc
    }

    /// The source map its spans resolve against.
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// The document and source map.
    pub fn into_parts(self) -> (KdlDocument, SourceMap) {
        (self.doc, self.sources)
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
    /// What is wrong.
    pub message: String,
    /// Where, with a short label.
    pub label: Label,
    /// How to fix it, when the parser knows.
    pub help: Option<String>,
}

impl ParseError {
    fn new(file: Arc<SourceFile>, err: &kdl::KdlError) -> Self {
        let mut problems: Vec<ParseProblem> = err
            .diagnostics
            .iter()
            .map(|d| ParseProblem {
                message: d.message.clone().unwrap_or_else(|| "invalid KDL".into()),
                label: Label::primary(
                    Span::new(file.id(), d.span.offset(), d.span.len()),
                    d.label.clone().unwrap_or_else(|| "here".into()),
                ),
                help: d.help.clone(),
            })
            .collect();
        if problems.is_empty() {
            problems.push(ParseProblem {
                message: "invalid KDL".into(),
                label: Label::primary(Span::new(file.id(), 0, 0), "here"),
                help: None,
            });
        }
        Self { file, problems }
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
    let doc = parse(sources.file(id))?;
    Ok(Loaded { doc, sources })
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
    let docs = map_maybe_parallel(sources.files(), |file| {
        let mut doc = parse(file)?;
        if file.base() > 0 {
            shift_doc(&mut doc, file.base());
        }
        Ok::<_, Error>(doc)
    });
    let mut merged = KdlDocument::new();
    for doc in docs {
        let mut doc = doc?;
        merged.nodes_mut().append(doc.nodes_mut());
    }
    Ok(Loaded {
        doc: merged,
        sources,
    })
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

fn parse(file: &Arc<SourceFile>) -> Result<KdlDocument, Error> {
    KdlDocument::parse_v2(file.text()).map_err(|e| ParseError::new(Arc::clone(file), &e).into())
}

/// Moves every span in `doc` by `by` bytes.
fn shift_doc(doc: &mut KdlDocument, by: usize) {
    let s = doc.span();
    doc.set_span((s.offset() + by, s.len()));
    for node in doc.nodes_mut() {
        shift_node(node, by);
    }
}

fn shift_node(node: &mut KdlNode, by: usize) {
    let s = node.span();
    node.set_span((s.offset() + by, s.len()));
    shift_ident(node.name_mut(), by);
    if let Some(ty) = node.ty_mut() {
        shift_ident(ty, by);
    }
    for entry in node.entries_mut() {
        let s = entry.span();
        entry.set_span((s.offset() + by, s.len()));
        if let Some(name) = entry.name_mut() {
            shift_ident(name, by);
        }
        if let Some(ty) = entry.ty_mut() {
            shift_ident(ty, by);
        }
    }
    if let Some(children) = node.children_mut() {
        shift_doc(children, by);
    }
}

fn shift_ident(ident: &mut kdl::KdlIdentifier, by: usize) {
    let s = ident.span();
    ident.set_span((s.offset() + by, s.len()));
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
        assert_eq!(flat.doc().nodes().len(), 2);

        let all = load_dir(dir.path(), true).unwrap();
        let sm = all.sources();
        let w = &all.doc().nodes()[2];
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
        let y = &all.doc().nodes()[1];
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
    }

    #[test]
    fn missing_dir_is_io() {
        let e = load_dir("/nonexistent/kdl-load-test", true).unwrap_err();
        assert_eq!(e.code(), "kdl_load::io");
    }
}
