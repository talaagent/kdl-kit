//! The error type shared by the deserializer and the serializer.
//!
//! Errors carry a stable [`code`](Error::code), a message, an optional help
//! text and labeled [`Span`]s, without depending on any renderer. With the
//! `miette` feature, [`Error`] is also a `miette::Diagnostic`.
//!
//! Deserializers record raw document offsets; the public entry points
//! resolve them against the [`SourceMap`] once an error escapes, so line and
//! column are only ever computed for errors that are reported.

use alloc::sync::Arc;
use core::fmt;

use kdl_span::{Label, Severity, SourceFile, SourceMap, Span};
use kdl_tree::Span as RawSpan;

/// A result with this crate's [`Error`].
pub type Result<T, E = Error> = core::result::Result<T, E>;

/// What went wrong, for callers that want to branch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input is not valid KDL v2 (code `kdl_load::parse`).
    Parse,
    /// Reading a file or directory failed (code `kdl_load::io`).
    Io,
    /// A struct field has no node, argument or property.
    MissingField,
    /// A node, argument or property matches no field (`deny_unknown_fields`).
    UnknownField,
    /// A field was given twice (e.g. as a property and as a child node).
    DuplicateField,
    /// A single-valued field was given by more than one node.
    DuplicateNode,
    /// Two named blocks share a name (code `kdl_nodes::duplicate_name`).
    DuplicateName,
    /// A node does not have the single name argument it needs (code
    /// `kdl_nodes::expected_identifier`).
    ExpectedIdentifier,
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

impl ErrorKind {
    /// The stable error code for this kind.
    pub fn code(self) -> &'static str {
        match self {
            Self::Parse => "kdl_load::parse",
            Self::Io => "kdl_load::io",
            Self::MissingField => "kdl_serde::missing_field",
            Self::UnknownField => "kdl_serde::unknown_field",
            Self::DuplicateField => "kdl_serde::duplicate_field",
            Self::DuplicateNode => "kdl_serde::duplicate_node",
            Self::DuplicateName => "kdl_nodes::duplicate_name",
            Self::ExpectedIdentifier => "kdl_nodes::expected_identifier",
            Self::UnknownVariant => "kdl_serde::unknown_variant",
            Self::InvalidType => "kdl_serde::invalid_type",
            Self::InvalidValue => "kdl_serde::invalid_value",
            Self::InvalidLength => "kdl_serde::invalid_length",
            Self::Unsupported => "kdl_serde::unsupported",
            Self::Custom => "kdl_serde::custom",
        }
    }

    fn default_label(self) -> &'static str {
        match self {
            Self::MissingField => "in this node",
            Self::UnknownField => "unknown field",
            Self::UnknownVariant => "unknown variant",
            Self::DuplicateNode | Self::DuplicateName => "duplicate",
            Self::DuplicateField => "given again here",
            Self::InvalidType => "wrong type",
            Self::InvalidValue => "invalid value",
            Self::InvalidLength => "wrong length",
            _ => "here",
        }
    }
}

/// A (de)serialization error.
///
/// [`Display`](fmt::Display) is the message alone; [`location`](Self::location)
/// gives `file:line:col`, and with the `miette` feature the error renders as
/// a diagnostic with a labeled source snippet:
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
#[derive(thiserror::Error)]
#[error("{}", self.inner.message)]
pub struct Error {
    inner: Box<Inner>,
    #[source]
    cause: Option<std::io::Error>,
}

struct Inner {
    kind: ErrorKind,
    message: String,
    label: Option<String>,
    help: Option<String>,
    /// Unresolved document offsets, until [`Error::resolve`].
    raw: Option<RawSpan>,
    raw_secondary: Vec<(RawSpan, String)>,
    labels: Vec<Label>,
    file: Option<Arc<SourceFile>>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            inner: Box::new(Inner {
                kind,
                message: message.into(),
                label: None,
                help: None,
                raw: None,
                raw_secondary: Vec::new(),
                labels: Vec::new(),
                file: None,
            }),
            cause: None,
        }
    }

    /// What kind of error this is.
    pub fn kind(&self) -> ErrorKind {
        self.inner.kind
    }

    /// The stable error code, e.g. `kdl_serde::missing_field`. See
    /// `docs/errors.md` in the repository for every code.
    pub fn code(&self) -> &'static str {
        self.inner.kind.code()
    }

    /// How serious the error is (always [`Severity::Error`]).
    pub fn severity(&self) -> Severity {
        Severity::Error
    }

    /// The error message, without location.
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    /// A suggested fix, if there is one.
    pub fn help(&self) -> Option<&str> {
        self.inner.help.as_deref()
    }

    /// The labeled spans, primary first. Empty when the error has no
    /// location (serialization errors, I/O errors).
    pub fn labels(&self) -> &[Label] {
        &self.inner.labels
    }

    /// The primary span, if any.
    pub fn span(&self) -> Option<Span> {
        self.inner.labels.first().map(|l| l.span)
    }

    /// The file the labels point into.
    pub fn source_file(&self) -> Option<&Arc<SourceFile>> {
        self.inner.file.as_ref()
    }

    /// The file's name, if the error has a location in a named file.
    pub fn file(&self) -> Option<&str> {
        self.source_file()
            .map(|f| f.name())
            .filter(|n| !n.is_empty())
    }

    /// The 1-based line and column of the primary span.
    pub fn line_col(&self) -> Option<(usize, usize)> {
        Some(self.source_file()?.line_col(self.span()?.offset))
    }

    /// `file:line:col` (or `line L column C` for unnamed input).
    pub fn location(&self) -> Option<String> {
        let (line, col) = self.line_col()?;
        Some(match self.file() {
            Some(file) => format!("{file}:{line}:{col}"),
            None => format!("line {line} column {col}"),
        })
    }

    pub(crate) fn with_label(mut self, label: impl Into<String>) -> Self {
        self.inner.label = Some(label.into());
        self
    }

    pub(crate) fn with_label_if_unset(mut self, label: &str) -> Self {
        if self.inner.raw.is_none() && self.inner.label.is_none() {
            self.inner.label = Some(label.to_owned());
        }
        self
    }

    pub(crate) fn with_help(mut self, help: impl Into<String>) -> Self {
        self.inner.help = Some(help.into());
        self
    }

    pub(crate) fn with_secondary(mut self, span: RawSpan, label: impl Into<String>) -> Self {
        self.inner.raw_secondary.push((span, label.into()));
        self
    }

    /// Rewords an unknown-field error raised for a property key.
    pub(crate) fn for_property(mut self) -> Self {
        if self.inner.kind == ErrorKind::UnknownField
            && let Some(rest) = self.inner.message.strip_prefix("unknown field")
        {
            self.inner.message = format!("unknown property{rest}");
            self.inner.label = Some("unknown property".to_owned());
        }
        self
    }

    pub(crate) fn has_span(&self) -> bool {
        self.inner.raw.is_some()
    }

    /// Points the error at a document span, unless a deeper deserializer
    /// already did.
    pub(crate) fn at(mut self, span: RawSpan) -> Self {
        if self.inner.raw.is_none() {
            self.inner.raw = Some(span);
        }
        self
    }

    /// Turns raw document offsets into file spans and labels.
    pub(crate) fn resolve(mut self, sources: &SourceMap) -> Self {
        let inner = &mut *self.inner;
        let Some(raw) = inner.raw.take() else {
            return self;
        };
        let Some(primary) = sources.resolve(raw) else {
            return self;
        };
        let label = inner
            .label
            .clone()
            .unwrap_or_else(|| inner.kind.default_label().to_owned());
        inner.labels.push(Label::primary(primary, label));
        for (span, label) in core::mem::take(&mut inner.raw_secondary) {
            let Some(span) = sources.resolve(span) else {
                continue;
            };
            if span.file == primary.file {
                inner.labels.push(Label::secondary(span, label));
            } else {
                let at = sources.describe(span);
                let what = label.trim_end_matches(" here");
                inner.help = Some(match inner.help.take() {
                    Some(help) => format!("{what} at {at}; {help}"),
                    None => format!("{what} at {at}"),
                });
            }
        }
        inner.file = Some(Arc::clone(sources.file(primary.file)));
        self
    }

    pub(crate) fn load(e: kdl_load::Error) -> Self {
        match e {
            kdl_load::Error::Parse(p) => {
                let first = &p.problems()[0];
                let mut err = Self::new(ErrorKind::Parse, first.message.clone());
                err.inner.labels.push(first.label.clone());
                err.inner.help.clone_from(&first.help);
                err.inner.file = Some(Arc::clone(p.file()));
                err
            }
            kdl_load::Error::Io { path, source } => {
                let mut err = Self::new(ErrorKind::Io, format!("cannot read {}", path.display()));
                err.cause = Some(source);
                err
            }
            other => Self::new(ErrorKind::Io, other.to_string()),
        }
    }

    pub(crate) fn nodes(e: kdl_nodes::Error, sources: &SourceMap) -> Self {
        let kind = match e.code() {
            "kdl_nodes::duplicate_name" => ErrorKind::DuplicateName,
            "kdl_nodes::expected_identifier" => ErrorKind::ExpectedIdentifier,
            _ => ErrorKind::Custom,
        };
        let mut err = Self::new(kind, e.to_string());
        err.inner.labels = e.labels().to_vec();
        err.inner.help = e.help().map(str::to_owned);
        err.inner.file = e
            .labels()
            .first()
            .map(|l| Arc::clone(sources.file(l.span.file)));
        err
    }

    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Error");
        d.field("code", &self.code())
            .field("message", &self.inner.message);
        if let Some(loc) = self.location() {
            d.field("at", &loc);
        }
        d.finish()
    }
}

#[cfg(feature = "miette")]
mod diagnostic {
    use super::Error;
    use core::fmt::Display;
    use miette::{Diagnostic, LabeledSpan, SourceCode};

    const DOCS: &str = "https://github.com/talaagent/kdl-serde/blob/main/docs/errors.md";

    impl Diagnostic for Error {
        fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            Some(Box::new(Error::code(self)))
        }

        fn url<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            let anchor = Error::code(self).replace("::", "-");
            Some(Box::new(format!("{DOCS}#{anchor}")))
        }

        fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
            self.help().map(|h| Box::new(h) as Box<dyn Display>)
        }

        fn source_code(&self) -> Option<&dyn SourceCode> {
            self.inner.file.as_deref().map(|f| f as &dyn SourceCode)
        }

        fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
            if self.inner.labels.is_empty() {
                return None;
            }
            Some(Box::new(
                self.inner.labels.iter().map(kdl_span::Label::to_miette),
            ))
        }
    }
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

fn display_field(name: &str) -> String {
    name.strip_prefix('@').unwrap_or(name).to_owned()
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
        if let Some(c) = kdl_span::did_you_mean(variant, expected.iter().copied()) {
            e = e.with_help(format!("did you mean `{c}`?"));
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
        if let Some(c) = kdl_span::did_you_mean(field, names.iter().copied()) {
            e = e.with_help(format!("did you mean `{c}`?"));
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
