//! The serializer: serde's data model to KDL nodes.
//!
//! Values first serialize to an intermediate [`Out`], which knows what a value
//! is (a scalar, a struct body, a sequence, a map, an enum variant) but not
//! yet the name of the node it will live in. [`Lower`] then turns an `Out`
//! into nodes for a given field name, which is where the conventions live:
//! scalar sequences become arguments, maps become `field key value` nodes,
//! variants become `(Variant)` annotations, and so on.

use alloc::string::ToString;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use serde::ser::{self, Serialize};

use crate::error::{Error, Result};
use crate::magic;

/// How a sequence of scalars is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SeqStyle {
    /// As arguments of one node: `tags "a" "b" "c"`. Sequences of structs
    /// are always repeated nodes.
    #[default]
    Args,
    /// As repeated nodes: `tags "a"`, `tags "b"`, `tags "c"`.
    Repeated,
}

/// How a map is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MapStyle {
    /// One node per entry, the key as the first argument:
    /// `env "HOME" "/root"`, `site "eu" { … }`.
    #[default]
    Keyed,
    /// One node whose children are the entries, named by key:
    /// `env { HOME "/root" }`.
    Block,
}

/// Serializer and formatter settings.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    /// How sequences of scalars are written.
    pub seq_style: SeqStyle,
    /// How maps are written.
    pub map_style: MapStyle,
    /// Sort map entries by key, so `HashMap`s print deterministically.
    pub sort_maps: bool,
    /// Spaces per indentation level in pretty output.
    pub indent: usize,
    /// Write strings that contain newlines as `"""` multiline strings.
    pub multiline_strings: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            seq_style: SeqStyle::default(),
            map_style: MapStyle::default(),
            sort_maps: true,
            indent: 2,
            multiline_strings: true,
        }
    }
}

impl Options {
    /// Sets [`seq_style`](Self::seq_style).
    #[must_use]
    pub fn seq_style(mut self, style: SeqStyle) -> Self {
        self.seq_style = style;
        self
    }

    /// Sets [`map_style`](Self::map_style).
    #[must_use]
    pub fn map_style(mut self, style: MapStyle) -> Self {
        self.map_style = style;
        self
    }

    /// Sets [`sort_maps`](Self::sort_maps).
    #[must_use]
    pub fn sort_maps(mut self, sort: bool) -> Self {
        self.sort_maps = sort;
        self
    }

    /// Sets [`indent`](Self::indent).
    #[must_use]
    pub fn indent(mut self, indent: usize) -> Self {
        self.indent = indent;
        self
    }

    /// Sets [`multiline_strings`](Self::multiline_strings).
    #[must_use]
    pub fn multiline_strings(mut self, multiline: bool) -> Self {
        self.multiline_strings = multiline;
        self
    }
}

/// A serialized value that has no node name yet.
#[derive(Debug, Clone)]
enum Out {
    /// A scalar, possibly with a `(type)` annotation.
    Value(KdlValue, Option<String>),
    /// `()`, a unit struct: an empty node.
    Unit,
    /// `None`: fields holding it are left out.
    None,
    /// A struct (or `RawDocument`): arguments, properties and children.
    Body(Body),
    Seq(Vec<Out>),
    Map(Vec<(KdlValue, Out)>),
    /// A unit, newtype, tuple or struct variant and its content.
    Variant(&'static str, Box<Out>),
    /// `Annotated<T>` around a non-scalar.
    Typed(String, Box<Out>),
    /// `RawNode`: a whole node, renamed to its field.
    Raw(Box<KdlNode>),
}

#[derive(Debug, Clone, Default)]
struct Body {
    /// `$name`: the node's name, when the struct carries it.
    name: Option<String>,
    /// `$type`: the node's annotation.
    ty: Option<String>,
    args: Vec<KdlEntry>,
    props: Vec<KdlEntry>,
    children: Vec<KdlNode>,
}

impl Out {
    /// The value as one argument, if it is a scalar.
    fn scalar(&self) -> Option<KdlEntry> {
        match self {
            Self::Value(v, ty) => {
                let mut e = KdlEntry::new(v.clone());
                if let Some(ty) = ty {
                    e.set_ty(ty.as_str());
                }
                Some(e)
            }
            Self::Variant(name, content) => match &**content {
                Self::Unit => Some(KdlEntry::new(*name)),
                Self::Value(v, None) => {
                    let mut e = KdlEntry::new(v.clone());
                    e.set_ty(*name);
                    Some(e)
                }
                Self::None => {
                    let mut e = KdlEntry::new(KdlValue::Null);
                    e.set_ty(*name);
                    Some(e)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

fn node_is_empty(n: &KdlNode) -> bool {
    n.entries().is_empty() && n.children().is_none_or(|c| c.nodes().is_empty())
}

fn looks_like_dash_block(n: &KdlNode) -> bool {
    n.entries().is_empty()
        && n.children().is_some_and(|c| {
            !c.nodes().is_empty() && c.nodes().iter().all(|c| c.name().value() == "-")
        })
}

/// Turns [`Out`]s into nodes.
struct Lower<'o> {
    opts: &'o Options,
}

impl Lower<'_> {
    /// The nodes a struct field `name` holding `out` becomes (none for `None`).
    fn field(&self, name: &str, out: Out, nodes: &mut Vec<KdlNode>) -> Result<()> {
        match out {
            Out::None => Ok(()),
            Out::Seq(items) => self.seq_field(name, items, nodes),
            Out::Map(entries) if self.opts.map_style == MapStyle::Keyed && !entries.is_empty() => {
                for (key, value) in entries {
                    let mut node = self.node(name, value)?;
                    node.entries_mut().insert(0, KdlEntry::new(key));
                    nodes.push(node);
                }
                Ok(())
            }
            out => {
                nodes.push(self.node(name, out)?);
                Ok(())
            }
        }
    }

    fn seq_field(&self, name: &str, items: Vec<Out>, nodes: &mut Vec<KdlNode>) -> Result<()> {
        if items.is_empty() {
            nodes.push(KdlNode::new(name));
            return Ok(());
        }
        let scalars: Option<Vec<KdlEntry>> = items.iter().map(Out::scalar).collect();
        if let Some(scalars) = scalars {
            match self.opts.seq_style {
                SeqStyle::Args => {
                    let mut node = KdlNode::new(name);
                    node.entries_mut().extend(scalars);
                    nodes.push(node);
                }
                SeqStyle::Repeated => nodes.extend(scalars.into_iter().map(|e| {
                    let mut node = KdlNode::new(name);
                    node.push(e);
                    node
                })),
            }
            return Ok(());
        }
        let as_nodes: Vec<KdlNode> = items
            .iter()
            .map(|i| self.node(name, i.clone()))
            .collect::<Result<_>>()?;
        // Repeated nodes read back as elements unless one is empty (an empty
        // field reads as an empty sequence) or a lone node looks like a
        // `{ - … }` element list. Those fall back to the dash form.
        let ambiguous = as_nodes.iter().any(node_is_empty)
            || (as_nodes.len() == 1 && looks_like_dash_block(&as_nodes[0]));
        if ambiguous {
            nodes.push(self.dash_block(name, items)?);
        } else {
            nodes.extend(as_nodes);
        }
        Ok(())
    }

    /// `name { - a; - b }`
    fn dash_block(&self, name: &str, items: Vec<Out>) -> Result<KdlNode> {
        let mut doc = KdlDocument::new();
        for item in items {
            let item = match item {
                Out::None => Out::Value(KdlValue::Null, None),
                other => other,
            };
            doc.nodes_mut().push(self.node("-", item)?);
        }
        let mut node = KdlNode::new(name);
        node.set_children(doc);
        Ok(node)
    }

    /// `out` as exactly one node called `name`.
    fn node(&self, name: &str, out: Out) -> Result<KdlNode> {
        let mut node = KdlNode::new(name);
        match out {
            Out::Value(..) => {
                node.push(
                    out.scalar()
                        .ok_or_else(|| Error::custom_internal("scalar"))?,
                );
            }
            Out::None => node.push(KdlEntry::new(KdlValue::Null)),
            Out::Unit => {}
            Out::Body(body) => {
                if let Some(ty) = body.ty {
                    node.set_ty(ty.as_str());
                }
                if let Some(n) = body.name {
                    node.set_name(n.as_str());
                }
                node.entries_mut().extend(body.args);
                node.entries_mut().extend(body.props);
                set_children(&mut node, body.children);
            }
            Out::Seq(items) => {
                let scalars: Option<Vec<KdlEntry>> = items.iter().map(Out::scalar).collect();
                match scalars {
                    Some(scalars) => node.entries_mut().extend(scalars),
                    None => return self.dash_block(name, items),
                }
            }
            Out::Map(entries) => {
                let mut children = Vec::new();
                for (key, value) in entries {
                    self.field(&key_name(&key)?, value, &mut children)?;
                }
                set_children(&mut node, children);
            }
            Out::Variant(variant, content) => return self.variant(name, variant, *content),
            Out::Typed(ty, inner) => {
                let mut inner = self.node(name, *inner)?;
                if inner.ty().is_some() {
                    return Err(Error::unsupported(format!(
                        "`{name}` already has a type annotation; cannot add `({ty})`"
                    )));
                }
                inner.set_ty(ty.as_str());
                return Ok(inner);
            }
            Out::Raw(mut raw) => {
                raw.set_name(name);
                return Ok(*raw);
            }
        }
        Ok(node)
    }

    /// A variant in a field: `name "Unit"`, `name (Newtype)1`,
    /// `(Struct)name { … }`, or `name { Variant … }` when the content
    /// already has an annotation.
    fn variant(&self, name: &str, variant: &'static str, content: Out) -> Result<KdlNode> {
        let out = Out::Variant(variant, Box::new(content));
        if let Some(e) = out.scalar() {
            let mut node = KdlNode::new(name);
            node.push(e);
            return Ok(node);
        }
        let Out::Variant(_, content) = out else {
            return Err(Error::custom_internal("variant"));
        };
        let inner_variant = matches!(*content, Out::Variant(..) | Out::Raw(_));
        let mut node = self.node(name, (*content).clone())?;
        if node.ty().is_none() && !inner_variant {
            node.set_ty(variant);
            return Ok(node);
        }
        let mut outer = KdlNode::new(name);
        set_children(&mut outer, vec![self.named(variant, *content)?]);
        Ok(outer)
    }

    /// A sequence element that names itself: a variant named by the
    /// variant, a struct with `$name` by that, anything else `-`.
    fn named(&self, name: &str, out: Out) -> Result<KdlNode> {
        match out {
            Out::Unit => Ok(KdlNode::new(name)),
            other => self.node(name, other),
        }
    }

    fn element(&self, out: Out) -> Result<KdlNode> {
        match out {
            Out::Variant(variant, content) => self.named(variant, *content),
            Out::Body(ref b) if b.name.is_some() => self.node("-", out),
            Out::Raw(raw) => Ok(*raw),
            Out::None => self.node("-", Out::Value(KdlValue::Null, None)),
            other => self.node("-", other),
        }
    }

    /// The top-level nodes of a document holding `out`.
    fn document(&self, out: Out) -> Result<KdlDocument> {
        let mut doc = KdlDocument::new();
        let nodes = doc.nodes_mut();
        match out {
            Out::Unit | Out::None => {}
            Out::Body(body) => {
                if !body.args.is_empty() || !body.props.is_empty() {
                    return Err(Error::unsupported(
                        "a document has no arguments or properties; `$N` and `@prop` fields only work inside a node",
                    ));
                }
                nodes.extend(body.children);
            }
            Out::Map(entries) => {
                for (key, value) in entries {
                    self.field(&key_name(&key)?, value, nodes)?;
                }
            }
            Out::Seq(items) => {
                for item in items {
                    nodes.push(self.element(item)?);
                }
            }
            Out::Variant(..) | Out::Raw(_) => nodes.push(self.element(out)?),
            Out::Value(..) | Out::Typed(..) => {
                return Err(Error::unsupported(
                    "a document must be a struct, map, sequence or enum, not a single value",
                ));
            }
        }
        Ok(doc)
    }
}

fn set_children(node: &mut KdlNode, children: Vec<KdlNode>) {
    if !children.is_empty() {
        let mut doc = KdlDocument::new();
        *doc.nodes_mut() = children;
        node.set_children(doc);
    }
}

fn key_name(key: &KdlValue) -> Result<String> {
    match key {
        KdlValue::String(s) => Ok(s.clone()),
        KdlValue::Integer(i) => Ok(i.to_string()),
        KdlValue::Bool(b) => Ok(b.to_string()),
        KdlValue::Float(f) => Ok(f.to_string()),
        KdlValue::Null => Err(Error::unsupported("a map key cannot be null")),
    }
}

fn compare_keys(a: &KdlValue, b: &KdlValue) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    match (a, b) {
        (KdlValue::String(a), KdlValue::String(b)) => a.cmp(b),
        (KdlValue::Integer(a), KdlValue::Integer(b)) => a.cmp(b),
        (KdlValue::Float(a), KdlValue::Float(b)) => a.total_cmp(b),
        (KdlValue::Bool(a), KdlValue::Bool(b)) => a.cmp(b),
        _ => a.to_string().cmp(&b.to_string()).then(Ordering::Equal),
    }
}

impl Error {
    fn custom_internal(what: &str) -> Self {
        <Self as ser::Error>::custom(format!("internal serializer error ({what})"))
    }
}

/// Builds a node body from struct fields, applying the field-name
/// conventions (`$0`, `$args`, `@prop`, `$name`, `$type`, `$children`).
fn body_from_fields(lower: &Lower<'_>, fields: Vec<(String, Out)>) -> Result<Body> {
    let mut body = Body::default();
    let mut positional: Vec<(usize, KdlEntry)> = Vec::new();
    let mut rest_args = Vec::new();
    for (key, out) in fields {
        if let Some(prop) = key.strip_prefix('@') {
            if matches!(out, Out::None) {
                continue;
            }
            let entry = out.scalar().ok_or_else(|| {
                Error::unsupported(format!("property `{prop}` must hold a scalar value"))
            })?;
            let mut entry = entry;
            entry.set_name(Some(prop));
            body.props.push(entry);
            continue;
        }
        match key.as_str() {
            "$args" => match out {
                Out::Seq(items) => {
                    for item in items {
                        rest_args.push(item.scalar().ok_or_else(|| {
                            Error::unsupported("`$args` must hold a sequence of scalars")
                        })?);
                    }
                }
                Out::None => {}
                _ => {
                    return Err(Error::unsupported(
                        "`$args` must hold a sequence of scalars",
                    ));
                }
            },
            "$name" => match out {
                Out::Value(KdlValue::String(s), _) => body.name = Some(s),
                _ => return Err(Error::unsupported("`$name` must hold a string")),
            },
            "$type" => match out {
                Out::Value(KdlValue::String(s), _) => body.ty = Some(s),
                Out::None => {}
                _ => return Err(Error::unsupported("`$type` must hold a string")),
            },
            "$children" => match out {
                Out::Seq(items) => {
                    for item in items {
                        body.children.push(lower.element(item)?);
                    }
                }
                Out::Map(entries) => {
                    for (key, value) in entries {
                        lower.field(&key_name(&key)?, value, &mut body.children)?;
                    }
                }
                Out::Body(b) => body.children.extend(b.children),
                Out::None | Out::Unit => {}
                _ => {
                    return Err(Error::unsupported(
                        "`$children` must hold a sequence or map",
                    ));
                }
            },
            _ => {
                if let Some(index) = key.strip_prefix('$').and_then(|i| i.parse::<usize>().ok()) {
                    if matches!(out, Out::None) {
                        continue;
                    }
                    let entry = out.scalar().ok_or_else(|| {
                        Error::unsupported(format!("argument `{key}` must hold a scalar value"))
                    })?;
                    positional.push((index, entry));
                } else {
                    lower.field(&key, out, &mut body.children)?;
                }
            }
        }
    }
    positional.sort_by_key(|(i, _)| *i);
    for (expected, (index, _)) in positional.iter().enumerate() {
        if *index != expected {
            return Err(Error::unsupported(format!(
                "argument `${expected}` is missing, so `${index}` cannot be written"
            )));
        }
    }
    body.args = positional.into_iter().map(|(_, e)| e).collect();
    body.args.extend(rest_args);
    Ok(body)
}

/// Serializes to a [`KdlDocument`].
pub(crate) fn to_document<T: ?Sized + Serialize>(value: &T, opts: &Options) -> Result<KdlDocument> {
    let out = value.serialize(OutSer { opts })?;
    Lower { opts }.document(out)
}

// ---------------------------------------------------------------------------
// The serde Serializer producing `Out`.

#[derive(Clone, Copy)]
struct OutSer<'o> {
    opts: &'o Options,
}

fn int(i: impl Into<i128>) -> Result<Out> {
    Ok(Out::Value(KdlValue::Integer(i.into()), None))
}

impl<'o> ser::Serializer for OutSer<'o> {
    type Ok = Out;
    type Error = Error;
    type SerializeSeq = SeqSer<'o>;
    type SerializeTuple = SeqSer<'o>;
    type SerializeTupleStruct = SeqSer<'o>;
    type SerializeTupleVariant = SeqSer<'o>;
    type SerializeMap = MapSer<'o>;
    type SerializeStruct = StructSer<'o>;
    type SerializeStructVariant = StructSer<'o>;

    fn serialize_bool(self, v: bool) -> Result<Out> {
        Ok(Out::Value(KdlValue::Bool(v), None))
    }
    fn serialize_i8(self, v: i8) -> Result<Out> {
        int(v)
    }
    fn serialize_i16(self, v: i16) -> Result<Out> {
        int(v)
    }
    fn serialize_i32(self, v: i32) -> Result<Out> {
        int(v)
    }
    fn serialize_i64(self, v: i64) -> Result<Out> {
        int(v)
    }
    fn serialize_i128(self, v: i128) -> Result<Out> {
        int(v)
    }
    fn serialize_u8(self, v: u8) -> Result<Out> {
        int(v)
    }
    fn serialize_u16(self, v: u16) -> Result<Out> {
        int(v)
    }
    fn serialize_u32(self, v: u32) -> Result<Out> {
        int(v)
    }
    fn serialize_u64(self, v: u64) -> Result<Out> {
        int(v)
    }
    fn serialize_u128(self, v: u128) -> Result<Out> {
        let i = i128::try_from(v)
            .map_err(|_| Error::unsupported(format!("{v} is too large for a KDL integer")))?;
        int(i)
    }
    fn serialize_f32(self, v: f32) -> Result<Out> {
        Ok(Out::Value(KdlValue::Float(f64::from(v)), None))
    }
    fn serialize_f64(self, v: f64) -> Result<Out> {
        Ok(Out::Value(KdlValue::Float(v), None))
    }
    fn serialize_char(self, v: char) -> Result<Out> {
        Ok(Out::Value(KdlValue::String(v.to_string()), None))
    }
    fn serialize_str(self, v: &str) -> Result<Out> {
        Ok(Out::Value(KdlValue::String(v.to_owned()), None))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Out> {
        Ok(Out::Value(
            KdlValue::String(crate::base64::encode(v)),
            Some("base64".to_owned()),
        ))
    }
    fn serialize_none(self) -> Result<Out> {
        Ok(Out::None)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Out> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Out> {
        Ok(Out::Unit)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Out> {
        Ok(Out::Unit)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Out> {
        Ok(Out::Variant(variant, Box::new(Out::Unit)))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Out> {
        match name {
            magic::RAW_NODE => {
                let text = value.serialize(OutSer { opts: self.opts })?;
                let Out::Value(KdlValue::String(text), _) = text else {
                    return Err(Error::custom_internal("raw node"));
                };
                let node: KdlNode = text
                    .parse()
                    .map_err(|_| Error::unsupported("RawNode holds invalid KDL"))?;
                Ok(Out::Raw(Box::new(node)))
            }
            magic::RAW_DOCUMENT => {
                let text = value.serialize(OutSer { opts: self.opts })?;
                let Out::Value(KdlValue::String(text), _) = text else {
                    return Err(Error::custom_internal("raw document"));
                };
                let doc: KdlDocument = text
                    .parse()
                    .map_err(|_| Error::unsupported("RawDocument holds invalid KDL"))?;
                Ok(Out::Body(Body {
                    children: doc.nodes().to_vec(),
                    ..Body::default()
                }))
            }
            _ => value.serialize(self),
        }
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Out> {
        Ok(Out::Variant(variant, Box::new(value.serialize(self)?)))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSer<'o>> {
        Ok(SeqSer {
            opts: self.opts,
            items: Vec::with_capacity(len.unwrap_or(0)),
            variant: None,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSer<'o>> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqSer<'o>> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SeqSer<'o>> {
        Ok(SeqSer {
            variant: Some(variant),
            ..self.serialize_seq(Some(len))?
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<MapSer<'o>> {
        // Maps of unknown length are structs with `#[serde(flatten)]`
        // fields; they follow the struct conventions.
        Ok(MapSer {
            opts: self.opts,
            entries: Vec::new(),
            key: None,
            as_struct: len.is_none(),
        })
    }

    fn serialize_struct(self, name: &'static str, len: usize) -> Result<StructSer<'o>> {
        Ok(StructSer {
            opts: self.opts,
            fields: Vec::with_capacity(len),
            variant: None,
            annotated: name == magic::ANNOTATED,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<StructSer<'o>> {
        Ok(StructSer {
            variant: Some(variant),
            ..self.serialize_struct("", len)?
        })
    }
}

struct SeqSer<'o> {
    opts: &'o Options,
    items: Vec<Out>,
    variant: Option<&'static str>,
}

impl SeqSer<'_> {
    fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let out = value.serialize(OutSer { opts: self.opts })?;
        self.items.push(match out {
            Out::None => Out::Value(KdlValue::Null, None),
            other => other,
        });
        Ok(())
    }

    fn finish(self) -> Out {
        let seq = Out::Seq(self.items);
        match self.variant {
            Some(v) => Out::Variant(v, Box::new(seq)),
            None => seq,
        }
    }
}

macro_rules! seq_impl {
    ($($tr:ident $m:ident;)*) => {$(
        impl ser::$tr for SeqSer<'_> {
            type Ok = Out;
            type Error = Error;
            fn $m<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
                self.push(value)
            }
            fn end(self) -> Result<Out> {
                Ok(self.finish())
            }
        }
    )*};
}

seq_impl! {
    SerializeSeq serialize_element;
    SerializeTuple serialize_element;
    SerializeTupleStruct serialize_field;
    SerializeTupleVariant serialize_field;
}

struct MapSer<'o> {
    opts: &'o Options,
    entries: Vec<(KdlValue, Out)>,
    key: Option<KdlValue>,
    as_struct: bool,
}

impl ser::SerializeMap for MapSer<'_> {
    type Ok = Out;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        let out = key.serialize(OutSer { opts: self.opts })?;
        let entry = out
            .scalar()
            .filter(|e| !e.value().is_null())
            .ok_or_else(|| Error::unsupported("map keys must be strings, numbers or booleans"))?;
        self.key = Some(entry.value().clone());
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let key = self
            .key
            .take()
            .ok_or_else(|| Error::custom_internal("map value without key"))?;
        let out = value.serialize(OutSer { opts: self.opts })?;
        self.entries.push((key, out));
        Ok(())
    }

    fn end(mut self) -> Result<Out> {
        if self.as_struct {
            let lower = Lower { opts: self.opts };
            let fields = self
                .entries
                .into_iter()
                .map(|(k, v)| Ok((key_name(&k)?, v)))
                .collect::<Result<_>>()?;
            return Ok(Out::Body(body_from_fields(&lower, fields)?));
        }
        if self.opts.sort_maps {
            self.entries.sort_by(|(a, _), (b, _)| compare_keys(a, b));
        }
        Ok(Out::Map(self.entries))
    }
}

struct StructSer<'o> {
    opts: &'o Options,
    fields: Vec<(String, Out)>,
    variant: Option<&'static str>,
    annotated: bool,
}

impl StructSer<'_> {
    fn finish(self) -> Result<Out> {
        if self.annotated {
            return annotate(self.fields);
        }
        let lower = Lower { opts: self.opts };
        let body = Out::Body(body_from_fields(&lower, self.fields)?);
        Ok(match self.variant {
            Some(v) => Out::Variant(v, Box::new(body)),
            None => body,
        })
    }
}

fn annotate(fields: Vec<(String, Out)>) -> Result<Out> {
    let mut ty = None;
    let mut value = Out::Unit;
    for (k, v) in fields {
        match (k.as_str(), v) {
            (magic::ANNOTATED_TYPE, Out::Value(KdlValue::String(s), _)) => ty = Some(s),
            (magic::ANNOTATED_TYPE, _) => {}
            (_, v) => value = v,
        }
    }
    let Some(ty) = ty else {
        return Ok(value);
    };
    Ok(match value {
        Out::Value(v, None) => Out::Value(v, Some(ty)),
        Out::None => Out::Value(KdlValue::Null, Some(ty)),
        Out::Body(mut b) if b.ty.is_none() => {
            b.ty = Some(ty);
            Out::Body(b)
        }
        other => Out::Typed(ty, Box::new(other)),
    })
}

impl ser::SerializeStruct for StructSer<'_> {
    type Ok = Out;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        let out = value.serialize(OutSer { opts: self.opts })?;
        self.fields.push((key.to_owned(), out));
        Ok(())
    }

    fn end(self) -> Result<Out> {
        self.finish()
    }
}

impl ser::SerializeStructVariant for StructSer<'_> {
    type Ok = Out;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        ser::SerializeStruct::serialize_field(self, key, value)
    }

    fn end(self) -> Result<Out> {
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dash_block_detection() {
        let n: KdlNode = "a { - 1; - 2 }".parse().unwrap();
        assert!(looks_like_dash_block(&n));
        let n: KdlNode = "a 1 { - 1 }".parse().unwrap();
        assert!(!looks_like_dash_block(&n));
    }
}
