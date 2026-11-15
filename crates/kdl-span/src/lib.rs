//! Source maps, spans and diagnostic building blocks for KDL documents.
//!
//! A [`SourceMap`] registers source files and gives each a [`FileId`] and a
//! range in one global offset space, so a document merged from several
//! files (see `kdl-load`) can keep plain global spans: [`SourceMap::resolve`]
//! (or [`SourceMap::span`]) turns a global `kdl-tree` span into a [`Span`]
//! (file, offset, length), [`SourceMap::node_name`] and friends take any
//! [`Spanned`] part of a document, and
//! [`SourceFile::line_col`] computes line and column on demand.
//!
//! Nothing here depends on `miette` unless the `miette` feature is on; then
//! [`SourceFile`] is a `miette::SourceCode` (named after its file) and
//! [`Span`]/[`Label`] convert to miette's span types.
//!
//! ```
//! use kdl_span::SourceMap;
//!
//! let mut map = SourceMap::new();
//! let a = map.add("a.kdl", "node 1\n");
//! let b = map.add("b.kdl", "x\ny 2\n");
//! let base = map.file(b).base();
//! let span = map.span(base + 4, 1).unwrap();
//! assert_eq!(span.file, b);
//! assert_eq!(map.file(span.file).line_col(span.offset), (2, 3));
//! assert_eq!(map.file(a).name(), "a.kdl");
//! ```

extern crate alloc;

use alloc::sync::Arc;
use std::sync::OnceLock;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// Identifies a file in a [`SourceMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    /// The file's index in its [`SourceMap`], in registration order.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A byte range in one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// The file the range is in.
    pub file: FileId,
    /// Byte offset from the start of the file.
    pub offset: usize,
    /// Length in bytes.
    pub len: usize,
}

impl Span {
    /// A span of `len` bytes at `offset` in `file`.
    pub fn new(file: FileId, offset: usize, len: usize) -> Self {
        Self { file, offset, len }
    }

    /// The span as miette's `SourceSpan` (file-local offsets).
    #[cfg(feature = "miette")]
    pub fn to_miette(self) -> miette::SourceSpan {
        (self.offset, self.len).into()
    }
}

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Severity {
    /// Must be fixed.
    #[default]
    Error,
    /// Suspicious but accepted.
    Warning,
    /// A hint.
    Advice,
}

/// A span with an optional message, as shown under a source snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Where.
    pub span: Span,
    /// What to say there.
    pub message: Option<String>,
    /// The main location of the diagnostic (others are context).
    pub primary: bool,
}

impl Label {
    /// The main label of a diagnostic.
    pub fn primary(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: Some(message.into()),
            primary: true,
        }
    }

    /// A context label.
    pub fn secondary(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: Some(message.into()),
            primary: false,
        }
    }

    /// The label as miette's `LabeledSpan`.
    #[cfg(feature = "miette")]
    pub fn to_miette(&self) -> miette::LabeledSpan {
        if self.primary {
            miette::LabeledSpan::new_primary_with_span(self.message.clone(), self.span.to_miette())
        } else {
            miette::LabeledSpan::new_with_span(self.message.clone(), self.span.to_miette())
        }
    }
}

/// One source file: its name, text and place in the global offset space.
#[derive(Debug)]
pub struct SourceFile {
    id: FileId,
    name: String,
    text: Arc<str>,
    base: usize,
    /// Byte offsets of line starts, computed on first use.
    lines: OnceLock<Vec<usize>>,
}

impl SourceFile {
    /// The file's id.
    pub fn id(&self) -> FileId {
        self.id
    }

    /// The file's name (usually its path; empty for unnamed input).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file's text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The file's text, shared.
    pub fn text_arc(&self) -> &Arc<str> {
        &self.text
    }

    /// Where the file starts in the [`SourceMap`]'s global offset space.
    pub fn base(&self) -> usize {
        self.base
    }

    fn line_starts(&self) -> &[usize] {
        self.lines.get_or_init(|| {
            core::iter::once(0)
                .chain(self.text.match_indices('\n').map(|(i, _)| i + 1))
                .collect()
        })
    }

    /// The 1-based line and column (in characters) of a file-local byte
    /// offset. Offsets past the end clamp to the end.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let starts = self.line_starts();
        let line = starts.partition_point(|&s| s <= offset).max(1);
        let start = starts[line - 1];
        let col = self
            .text
            .get(start..offset)
            .map_or(offset - start, |s| s.chars().count());
        (line, col + 1)
    }
}

#[cfg(feature = "miette")]
impl miette::SourceCode for SourceFile {
    fn read_span<'a>(
        &'a self,
        span: &miette::SourceSpan,
        before: usize,
        after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        let contents = self.text.read_span(span, before, after)?;
        if self.name.is_empty() {
            return Ok(contents);
        }
        Ok(Box::new(miette::MietteSpanContents::new_named(
            self.name.clone(),
            contents.data(),
            *contents.span(),
            contents.line(),
            contents.column(),
            contents.line_count(),
        )))
    }
}

/// The files a document came from, laid out in one offset space.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    files: Vec<Arc<SourceFile>>,
    end: usize,
}

impl SourceMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a file and returns its id. Its global offsets start where
    /// the previous file ended, plus one, so spans never straddle files.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<Arc<str>>) -> FileId {
        let text: Arc<str> = text.into();
        let id = FileId(u32::try_from(self.files.len()).unwrap_or(u32::MAX));
        let base = if self.files.is_empty() {
            0
        } else {
            self.end + 1
        };
        self.end = base + text.len();
        self.files.push(Arc::new(SourceFile {
            id,
            name: name.into(),
            text,
            base,
            lines: OnceLock::new(),
        }));
        id
    }

    /// The file with id `id`.
    ///
    /// # Panics
    ///
    /// If `id` is from another map.
    pub fn file(&self, id: FileId) -> &Arc<SourceFile> {
        &self.files[id.index()]
    }

    /// All files, in registration order.
    pub fn files(&self) -> &[Arc<SourceFile>] {
        &self.files
    }

    /// The file containing global offset `offset`.
    pub fn file_at(&self, offset: usize) -> Option<&Arc<SourceFile>> {
        let i = self.files.partition_point(|f| f.base <= offset);
        let file = self.files.get(i.checked_sub(1)?)?;
        (offset <= file.base + file.text.len()).then_some(file)
    }

    /// Converts a global offset and length (a span in a document
    /// loaded through this map) to a file-local [`Span`].
    pub fn span(&self, offset: usize, len: usize) -> Option<Span> {
        let file = self.file_at(offset)?;
        Some(Span::new(file.id, offset - file.base, len))
    }

    /// Converts a span from a document loaded through this map (a
    /// `kdl-tree` span, in the global offset space) to a file-local
    /// [`Span`].
    pub fn resolve(&self, span: kdl_tree::Span) -> Option<Span> {
        self.span(span.offset, span.len)
    }

    /// The span of a node's name, the usual place to point at a node.
    pub fn node_name(&self, node: &(impl SpannedNode + ?Sized)) -> Option<Span> {
        self.resolve(node.name_span())
    }

    /// The span of a whole node, children included.
    pub fn node(&self, node: &(impl Spanned + ?Sized)) -> Option<Span> {
        self.resolve(node.global_span())
    }

    /// The span of an argument or property.
    pub fn entry(&self, entry: &(impl Spanned + ?Sized)) -> Option<Span> {
        self.resolve(entry.global_span())
    }

    /// The span of an identifier (a node name, property key or annotation).
    pub fn ident(&self, ident: &(impl Spanned + ?Sized)) -> Option<Span> {
        self.resolve(ident.global_span())
    }

    /// `file:line:col` for a span, for plain-text messages.
    pub fn describe(&self, span: Span) -> String {
        let file = self.file(span.file);
        let (line, col) = file.line_col(span.offset);
        if file.name().is_empty() {
            format!("line {line} column {col}")
        } else {
            format!("{}:{line}:{col}", file.name())
        }
    }
}

/// A part of a document with a global span: `kdl-tree`'s nodes, entries,
/// identifiers and documents.
pub trait Spanned {
    /// The span in the source map's global offset space.
    fn global_span(&self) -> kdl_tree::Span;
}

/// A node: [`Spanned`], with a name span.
pub trait SpannedNode: Spanned {
    /// The span of the node's name.
    fn name_span(&self) -> kdl_tree::Span;
}

impl Spanned for kdl_tree::Node<'_> {
    fn global_span(&self) -> kdl_tree::Span {
        self.span()
    }
}

impl SpannedNode for kdl_tree::Node<'_> {
    fn name_span(&self) -> kdl_tree::Span {
        self.name().span()
    }
}

impl Spanned for kdl_tree::Entry<'_> {
    fn global_span(&self) -> kdl_tree::Span {
        self.span()
    }
}

impl Spanned for kdl_tree::Ident<'_> {
    fn global_span(&self) -> kdl_tree::Span {
        self.span()
    }
}

impl Spanned for kdl_tree::Document<'_> {
    fn global_span(&self) -> kdl_tree::Span {
        self.span()
    }
}

/// Edit distance counting an adjacent transposition as one edit.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}

/// The candidate closest to `name`, if it is close enough to be a typo.
pub fn did_you_mean<'c>(
    name: &str,
    candidates: impl IntoIterator<Item = &'c str>,
) -> Option<&'c str> {
    candidates
        .into_iter()
        .map(|c| (edit_distance(name, c), c))
        .filter(|(d, c)| *d <= (c.len().max(name.len()) / 3).max(1))
        .min()
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col() {
        let mut map = SourceMap::new();
        let id = map.add("f", "ab\ncdé\n\nx");
        let f = map.file(id);
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(2), (1, 3));
        assert_eq!(f.line_col(3), (2, 1));
        assert_eq!(f.line_col(7), (2, 4));
        assert_eq!(f.line_col(9), (4, 1));
        assert_eq!(f.line_col(99), (4, 2));
    }

    #[test]
    fn files_do_not_overlap() {
        let mut map = SourceMap::new();
        let a = map.add("a", "123");
        let b = map.add("b", "");
        let c = map.add("c", "xy");
        assert_eq!(map.span(3, 0).map(|s| s.file), Some(a));
        assert_eq!(map.span(4, 0).map(|s| s.file), Some(b));
        assert_eq!(map.span(5, 1), Some(Span::new(c, 0, 1)));
        assert_eq!(map.span(99, 0), None);
    }

    #[test]
    fn suggestions() {
        assert_eq!(did_you_mean("prot", ["host", "port"]), Some("port"));
        assert_eq!(did_you_mean("zzzz", ["host", "port"]), None);
    }
}
