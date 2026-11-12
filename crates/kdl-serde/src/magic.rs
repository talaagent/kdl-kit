//! Struct and newtype names the deserializer and serializer treat specially.

/// [`Annotated`](crate::Annotated): a value and its `(type)` annotation.
pub(crate) const ANNOTATED: &str = "$kdl_serde::Annotated";
pub(crate) const ANNOTATED_TYPE: &str = "$type";
pub(crate) const ANNOTATED_VALUE: &str = "$value";
pub(crate) const ANNOTATED_FIELDS: &[&str] = &[ANNOTATED_TYPE, ANNOTATED_VALUE];

/// [`RawNode`](crate::RawNode): a node passed through as KDL text.
pub(crate) const RAW_NODE: &str = "$kdl_serde::RawNode";

/// [`RawDocument`](crate::RawDocument): a document or children block passed
/// through as KDL text.
pub(crate) const RAW_DOCUMENT: &str = "$kdl_serde::RawDocument";

pub(crate) fn is_raw(name: &str) -> bool {
    name == RAW_NODE || name == RAW_DOCUMENT
}
