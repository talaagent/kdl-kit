//! Parse errors.

use alloc::borrow::Cow;

use crate::Span;

/// What kind of syntax error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Something other than what the grammar allows here (a missing node
    /// name, value, `)`, terminator or whitespace; a stray `}`).
    Unexpected,
    /// A string, comment or children block runs to the end of the input.
    Unterminated,
    /// A code point KDL forbids anywhere (control characters, bidi
    /// overrides, a BOM after the start).
    DisallowedChar,
    /// A malformed or out-of-range number.
    InvalidNumber,
    /// A bare identifier that is a keyword (`true`) or looks like a number.
    InvalidIdentifier,
    /// An unknown `#keyword`.
    InvalidKeyword,
    /// A malformed string: a newline in a single-line string, or bad
    /// multi-line indentation.
    InvalidString,
    /// An unknown or malformed `\` escape.
    InvalidEscape,
    /// An entry after a children block, or two children blocks.
    ChildrenOrder,
    /// Children blocks nested deeper than [`ParseOptions::max_depth`](crate::ParseOptions::max_depth).
    TooDeep,
}

impl ErrorKind {
    /// The stable error code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unexpected => "kdl_tree::unexpected",
            Self::Unterminated => "kdl_tree::unterminated",
            Self::DisallowedChar => "kdl_tree::disallowed_char",
            Self::InvalidNumber => "kdl_tree::invalid_number",
            Self::InvalidIdentifier => "kdl_tree::invalid_identifier",
            Self::InvalidKeyword => "kdl_tree::invalid_keyword",
            Self::InvalidString => "kdl_tree::invalid_string",
            Self::InvalidEscape => "kdl_tree::invalid_escape",
            Self::ChildrenOrder => "kdl_tree::children_order",
            Self::TooDeep => "kdl_tree::too_deep",
        }
    }
}

/// A KDL syntax error at a byte offset.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    kind: ErrorKind,
    span: Span,
    message: Cow<'static, str>,
    help: Option<&'static str>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, span: Span, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            span,
            message: message.into(),
            help: None,
        }
    }

    pub(crate) fn with_help(mut self, help: &'static str) -> Self {
        self.help = Some(help);
        self
    }

    /// What kind of error this is.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The stable error code, e.g. `kdl_tree::unterminated`.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// Where: usually the offending character (empty at the end of input);
    /// the opening delimiter for something unterminated.
    pub fn span(&self) -> Span {
        self.span
    }

    /// What is wrong, without position.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// How to fix it, when the parser knows.
    pub fn help(&self) -> Option<&'static str> {
        self.help
    }
}
