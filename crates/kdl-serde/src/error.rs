//! The error type shared by the deserializer and the serializer.

use alloc::sync::Arc;
use core::fmt;
use std::path::PathBuf;

use miette::{Diagnostic, LabeledSpan, MietteError, SourceCode, SourceSpan, SpanContents};

/// A result with this crate's [`Error`].
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// What went wrong, for callers that want to branch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input is not valid KDL v2.
    Parse,
    /// Reading a file or directory failed.
    Io,
    /// A struct field has no node, argument or property.
    MissingField,
    /// A node, argument or property matches no field (`deny_unknown_fields`).
    UnknownField,
    /// A field was given twice (e.g. as a property and as a child node).
    DuplicateField,
    /// A single-valued field was given by more than one node.
    DuplicateNode,
    /// An enum variant name is not one of the enum's variants.
    UnknownVariant,
    /// A value has the wrong type (a string where a number was expected, …).
    InvalidType,
    /// A value has the right type but is out of range or otherwise invalid.
    InvalidValue,
    /// A sequence or tuple has the wrong number of elements.
    InvalidLength,
    /// The value cannot be represented in KDL (serializer only).
    Unsupported,
    /// Any other error, including `Error::custom` from user code.
    Custom,
}

/// A source document: its text and, when it came from a file, its path.
#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) name: Option<String>,
    pub(crate) text: Arc<String>,
}

impl Source {
    pub(crate) fn new(name: Option<String>, text: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            name,
            text: Arc::new(text.into()),
        })
    }
}

/// [`Source`] as miette source code, named after its file when it has one.
#[derive(Debug, Clone)]
struct SourceRef(Arc<Source>);

impl SourceCode for SourceRef {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        before: usize,
        after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        let contents = self.0.text.read_span(span, before, after)?;
        Ok(match &self.0.name {
            Some(name) => Box::new(miette::MietteSpanContents::new_named(
                name.clone(),
                contents.data(),
                *contents.span(),
                contents.line(),
                contents.column(),
                contents.line_count(),
            )),
            None => contents,
        })
    }
}

/// A (de)serialization error.
///
/// Deserialization errors carry the source text, the file name (for
/// [`from_dir`](crate::from_dir) and [`from_file`](crate::from_file)) and a
/// labeled span, and render as a [`miette`] diagnostic:
///
/// ```text
///   × unknown field `prot`, expected one of `host`, `port`
///    ╭─[config.kdl:2:1]
///  1 │ host "localhost"
///  2 │ prot 8080
///    · ──┬─
///    ·   ╰── unknown field
///    ╰────
///   help: did you mean `port`?
/// ```
pub struct Error {
    inner: Box<Inner>,
}

struct Inner {
    kind: ErrorKind,
    message: String,
    label: Option<String>,
    help: Option<String>,
    span: Option<SourceSpan>,
    secondary: Vec<(SourceSpan, String)>,
    source: Option<SourceRef>,
    path: Option<PathBuf>,
    cause: Option<Box<dyn core::error::Error + Send + Sync>>,
    related: Vec<Error>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            inner: Box::new(Inner {
                kind,
                message: message.into(),
                label: None,
                help: None,
                span: None,
                secondary: Vec::new(),
                source: None,
                path: None,
                cause: None,
                related: Vec::new(),
            }),
        }
    }

    /// What kind of error this is.
    pub fn kind(&self) -> ErrorKind {
        self.inner.kind
    }

    /// The error message, without location.
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    /// The byte span in the source the error points at, if any.
    pub fn span(&self) -> Option<SourceSpan> {
        self.inner.span
    }

    /// The file the error happened in, if the input came from a file.
    pub fn file(&self) -> Option<&str> {
        self.inner.source.as_ref()?.0.name.as_deref()
    }

    /// The 1-based line and column of [`span`](Self::span), if any.
    pub fn line_col(&self) -> Option<(usize, usize)> {
        let offset = self.inner.span?.offset();
        let text = self.inner.source.as_ref()?.0.text.as_str();
        let before = text.get(..offset)?;
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        Some((line, col))
    }

    pub(crate) fn with_label(mut self, label: impl Into<String>) -> Self {
        self.inner.label = Some(label.into());
        self
    }

    pub(crate) fn with_label_if_unset(mut self, label: &str) -> Self {
        if self.inner.span.is_none() && self.inner.label.is_none() {
            self.inner.label = Some(label.to_owned());
        }
        self
    }

    pub(crate) fn with_help(mut self, help: impl Into<String>) -> Self {
        self.inner.help = Some(help.into());
        self
    }

    pub(crate) fn with_secondary(mut self, span: SourceSpan, label: impl Into<String>) -> Self {
        self.inner.secondary.push((span, label.into()));
        self
    }

    /// Rewords an unknown-field error raised for a property key.
    pub(crate) fn for_property(mut self) -> Self {
        if self.inner.kind == ErrorKind::UnknownField {
            if let Some(rest) = self.inner.message.strip_prefix("unknown field") {
                self.inner.message = format!("unknown property{rest}");
                self.inner.label = Some("unknown property".to_owned());
            }
        }
        self
    }

    pub(crate) fn has_span(&self) -> bool {
        self.inner.span.is_some()
    }

    /// Points the error at `span` in `source`, unless a deeper deserializer
    /// already did.
    pub(crate) fn at(mut self, source: &Arc<Source>, span: SourceSpan) -> Self {
        if self.inner.span.is_none() {
            self.inner.span = Some(span);
            self.inner.source = Some(SourceRef(Arc::clone(source)));
        }
        self
    }

    pub(crate) fn io(path: PathBuf, err: std::io::Error) -> Self {
        let mut e = Self::new(ErrorKind::Io, format!("{}: {err}", path.display()));
        e.inner.path = Some(path);
        e.inner.cause = Some(Box::new(err));
        e
    }

    pub(crate) fn parse(source: &Arc<Source>, err: &kdl::KdlError) -> Self {
        let mut diags = err.diagnostics.iter().map(|d| {
            let mut e = Self::new(
                ErrorKind::Parse,
                d.message.clone().unwrap_or_else(|| "invalid KDL".into()),
            )
            .with_label(d.label.clone().unwrap_or_else(|| "here".into()))
            .at(source, d.span);
            e.inner.help.clone_from(&d.help);
            e
        });
        let mut first = diags.next().unwrap_or_else(|| {
            Self::new(ErrorKind::Parse, "invalid KDL").at(source, (0, 0).into())
        });
        first.inner.related = diags.collect();
        first
    }

    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    fn primary_label(&self) -> String {
        if let Some(l) = &self.inner.label {
            return l.clone();
        }
        match self.inner.kind {
            ErrorKind::MissingField => "in this node",
            ErrorKind::UnknownField => "unknown field",
            ErrorKind::UnknownVariant => "unknown variant",
            ErrorKind::DuplicateNode => "duplicate",
            ErrorKind::DuplicateField => "given again here",
            ErrorKind::InvalidType => "wrong type",
            ErrorKind::InvalidValue => "invalid value",
            ErrorKind::InvalidLength => "wrong length",
            _ => "here",
        }
        .to_owned()
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Error");
        d.field("kind", &self.inner.kind)
            .field("message", &self.inner.message);
        if let Some(file) = self.file() {
            d.field("file", &file);
        }
        if let Some((line, col)) = self.line_col() {
            d.field("line", &line).field("col", &col);
        }
        d.finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner.message)?;
        match (self.file(), self.line_col()) {
            (Some(file), Some((line, col))) => write!(f, " at {file}:{line}:{col}"),
            (None, Some((line, col))) => write!(f, " at line {line} column {col}"),
            (Some(file), None) => write!(f, " in {file}"),
            (None, None) => Ok(()),
        }
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.inner
            .cause
            .as_deref()
            .map(|e| e as &(dyn core::error::Error + 'static))
    }
}

impl Diagnostic for Error {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        let code = match self.inner.kind {
            ErrorKind::Parse => "kdl_serde::parse",
            ErrorKind::Io => "kdl_serde::io",
            ErrorKind::MissingField => "kdl_serde::missing_field",
            ErrorKind::UnknownField => "kdl_serde::unknown_field",
            ErrorKind::DuplicateField => "kdl_serde::duplicate_field",
            ErrorKind::DuplicateNode => "kdl_serde::duplicate_node",
            ErrorKind::UnknownVariant => "kdl_serde::unknown_variant",
            ErrorKind::InvalidType => "kdl_serde::invalid_type",
            ErrorKind::InvalidValue => "kdl_serde::invalid_value",
            ErrorKind::InvalidLength => "kdl_serde::invalid_length",
            ErrorKind::Unsupported => "kdl_serde::unsupported",
            ErrorKind::Custom => "kdl_serde::custom",
        };
        Some(Box::new(code))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.inner
            .help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.inner.span?;
        self.inner.source.as_ref().map(|s| s as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let span = self.inner.span?;
        let primary = LabeledSpan::new_primary_with_span(Some(self.primary_label()), span);
        let rest = self
            .inner
            .secondary
            .iter()
            .map(|(s, l)| LabeledSpan::new_with_span(Some(l.clone()), *s));
        Some(Box::new(core::iter::once(primary).chain(rest)))
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        if self.inner.related.is_empty() {
            return None;
        }
        Some(Box::new(
            self.inner.related.iter().map(|e| e as &dyn Diagnostic),
        ))
    }
}

/// Edit distance counting adjacent transpositions as one edit, for "did
/// you mean" hints.
fn distance(a: &str, b: &str) -> usize {
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

fn did_you_mean(name: &str, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .map(|c| (distance(name, c), *c))
        .filter(|(d, c)| *d <= (c.len().max(name.len()) / 3).max(1))
        .min()
        .map(|(_, c)| format!("did you mean `{}`?", display_field(c)))
}

/// How a field name reads in KDL: `$0` is "argument 0", `@key` "property key".
fn display_field(name: &str) -> String {
    name.strip_prefix('@').unwrap_or(name).to_owned()
}

fn one_of(names: &[&str]) -> String {
    match names {
        [] => "no fields".to_owned(),
        [one] => format!("`{one}`"),
        _ => {
            let list: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
            format!("one of {}", list.join(", "))
        }
    }
}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::new(ErrorKind::Custom, msg.to_string())
    }

    fn invalid_type(unexp: serde::de::Unexpected<'_>, exp: &dyn serde::de::Expected) -> Self {
        Self::new(
            ErrorKind::InvalidType,
            format!("invalid type: {unexp}, expected {exp}"),
        )
        .with_label(format!("expected {exp}"))
    }

    fn invalid_value(unexp: serde::de::Unexpected<'_>, exp: &dyn serde::de::Expected) -> Self {
        Self::new(
            ErrorKind::InvalidValue,
            format!("invalid value: {unexp}, expected {exp}"),
        )
        .with_label(format!("expected {exp}"))
    }

    fn invalid_length(len: usize, exp: &dyn serde::de::Expected) -> Self {
        Self::new(
            ErrorKind::InvalidLength,
            format!("invalid length {len}, expected {exp}"),
        )
        .with_label(format!("expected {exp}"))
    }

    fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Self {
        let mut e = Self::new(
            ErrorKind::UnknownVariant,
            format!("unknown variant `{variant}`, expected {}", one_of(expected)),
        );
        if let Some(help) = did_you_mean(variant, expected) {
            e = e.with_help(help);
        }
        e
    }

    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
        if field.starts_with('$') {
            let takes = expected
                .iter()
                .filter(|f| {
                    f.strip_prefix('$')
                        .is_some_and(|i| i.parse::<usize>().is_ok())
                })
                .count();
            let takes = match takes {
                0 => "no arguments".to_owned(),
                1 => "1 argument".to_owned(),
                n => format!("{n} arguments"),
            };
            return Self::new(
                ErrorKind::UnknownField,
                format!("unexpected argument; this node takes {takes}"),
            )
            .with_label("unexpected argument");
        }
        let names: Vec<&str> = expected
            .iter()
            .filter(|f| !f.starts_with('$'))
            .map(|f| f.strip_prefix('@').unwrap_or(f))
            .collect();
        let mut e = Self::new(
            ErrorKind::UnknownField,
            format!("unknown field `{field}`, expected {}", one_of(&names)),
        );
        if let Some(help) = did_you_mean(field, &names) {
            e = e.with_help(help);
        }
        e
    }

    fn missing_field(field: &'static str) -> Self {
        let what = match field.as_bytes().first() {
            Some(b'$') => match field[1..].parse::<usize>() {
                Ok(i) => format!("missing argument {}", i + 1),
                Err(_) => format!("missing `{field}`"),
            },
            Some(b'@') => format!("missing property `{}`", &field[1..]),
            _ => format!("missing field `{field}`"),
        };
        Self::new(ErrorKind::MissingField, what)
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self::new(
            ErrorKind::DuplicateField,
            format!("duplicate field `{}`", display_field(field)),
        )
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::new(ErrorKind::Custom, msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_names() {
        assert_eq!(
            did_you_mean("prot", &["host", "port"]).as_deref(),
            Some("did you mean `port`?")
        );
        assert_eq!(did_you_mean("zzzzzz", &["host", "port"]), None);
    }
}
