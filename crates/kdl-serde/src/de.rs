//! The deserializer: KDL nodes to serde's data model.
//!
//! Four deserializers cover the shapes a value can take in a document:
//!
//! - [`BlockDe`]: a list of sibling nodes (the document, or a children
//!   block), read as a struct or map keyed by node name, or a sequence;
//! - [`GroupDe`]: the nodes that share one name in a block, i.e. one struct
//!   field (`member …` repeated is one group);
//! - [`NodeDe`]: a single node, read as a scalar (its one argument), a struct
//!   (arguments, properties and children), a sequence (its arguments) and
//!   so on;
//! - [`ValueDe`]: a single argument or property value.
//!
//! Errors raised by visitors carry no location; each deserializer stamps its
//! own span on errors that come back without one, so the innermost span wins.

use alloc::borrow::Cow;

use kdl_tree::{Document, Entry, Node, Span as RawSpan, Value};
use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, Error as _, IntoDeserializer, MapAccess,
    SeqAccess, Unexpected, VariantAccess, Visitor,
};

use crate::error::{Error, ErrorKind, Result};
use crate::magic;

/// A node and the source it came from.
#[derive(Clone, Copy)]
pub(crate) struct NodeRef<'a> {
    pub(crate) node: &'a Node<'a>,
}

impl<'a> NodeRef<'a> {
    fn name(self) -> &'a str {
        self.node.name().value()
    }

    fn name_span(self) -> RawSpan {
        self.node.name().span()
    }

    fn children(self) -> impl Iterator<Item = NodeRef<'a>> {
        self.node
            .children()
            .into_iter()
            .flat_map(kdl_tree::Document::nodes)
            .map(move |node| NodeRef { node })
    }

    fn has_children(self) -> bool {
        self.node.children().is_some_and(|d| !d.nodes().is_empty())
    }
}

fn is_dash_block(n: NodeRef<'_>) -> bool {
    n.has_children() && n.children().all(|c| c.name() == "-")
}

// ---------------------------------------------------------------------------
// ValueDe: one argument or property value.

/// A single KDL value (an argument or a property value).
#[derive(Clone, Copy)]
pub(crate) struct ValueDe<'a> {
    entry: &'a Entry<'a>,
    /// The `(type)` annotation was taken by an enum or `Annotated`.
    ty_used: bool,
}

impl<'a> ValueDe<'a> {
    fn new(entry: &'a Entry<'a>) -> Self {
        Self {
            entry,
            ty_used: false,
        }
    }

    fn ty(&self) -> Option<&'a str> {
        if self.ty_used {
            return None;
        }
        self.entry.ty().map(kdl_tree::Ident::value)
    }

    fn wrap<T>(&self, r: Result<T>) -> Result<T> {
        r.map_err(|e| e.at(self.entry.span()))
    }

    fn unexpected(&self) -> Unexpected<'a> {
        match self.entry.value() {
            Value::String(s) => Unexpected::Str(s),
            Value::Integer(i) => match i64::try_from(*i) {
                Ok(i) => Unexpected::Signed(i),
                Err(_) => Unexpected::Other("a large integer"),
            },
            Value::Float(f) => Unexpected::Float(*f),
            Value::Bool(b) => Unexpected::Bool(*b),
            Value::Null => Unexpected::Unit,
        }
    }

    fn visit<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let r = match self.entry.value() {
            Value::String(s) => visitor.visit_borrowed_str(s),
            Value::Integer(i) => visit_int(*i, visitor),
            Value::Float(f) => visitor.visit_f64(*f),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Null => visitor.visit_unit(),
        };
        self.wrap(r)
    }
}

fn visit_int<'de, V: Visitor<'de>>(i: i128, visitor: V) -> Result<V::Value> {
    if let Ok(i) = i64::try_from(i) {
        visitor.visit_i64(i)
    } else if let Ok(u) = u64::try_from(i) {
        visitor.visit_u64(u)
    } else {
        visitor.visit_i128(i)
    }
}

macro_rules! value_forward {
    ($($m:ident)*) => {$(
        fn $m<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
            self.visit(visitor)
        }
    )*};
}

impl<'a> Deserializer<'a> for ValueDe<'a> {
    type Error = Error;

    value_forward! {
        deserialize_any deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32
        deserialize_i64 deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32
        deserialize_u64 deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char
        deserialize_str deserialize_string deserialize_identifier deserialize_ignored_any
    }

    fn deserialize_unit<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        match self.entry.value() {
            Value::Null => self.wrap(visitor.visit_unit()),
            _ => self.wrap(Err(Error::invalid_type(self.unexpected(), &visitor))),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        match self.entry.value() {
            Value::Null => self.wrap(visitor.visit_none()),
            _ => self.wrap(visitor.visit_some(self)),
        }
    }

    fn deserialize_bytes<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        match self.entry.value() {
            Value::String(s) => match crate::base64::decode(s) {
                Some(bytes) => self.wrap(visitor.visit_byte_buf(bytes)),
                None => self.wrap(Err(Error::invalid_value(
                    Unexpected::Str(s),
                    &"base64-encoded bytes",
                ))),
            },
            _ => self.wrap(Err(Error::invalid_type(self.unexpected(), &visitor))),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        if magic::is_raw(name) {
            return self.wrap(Err(Error::invalid_type(self.unexpected(), &"a KDL node")));
        }
        self.wrap(visitor.visit_newtype_struct(self))
    }

    fn deserialize_seq<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        self.wrap(Err(Error::invalid_type(self.unexpected(), &visitor)))
    }

    fn deserialize_tuple<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        if name == magic::ANNOTATED {
            let ty = self
                .ty()
                .map(|t| (t, self.entry.ty().map(kdl_tree::Ident::span)));
            let value = ValueDe {
                ty_used: true,
                ..self
            };
            return self.wrap(visitor.visit_map(AnnotatedAccess::new(ty, value)));
        }
        self.deserialize_seq(visitor)
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        if let (Some(ty), Some(ident)) = (self.ty(), self.entry.ty()) {
            let tag = Tag::new(ty, ident.span());
            let content = ValueDe {
                ty_used: true,
                ..self
            };
            return self.wrap(visitor.visit_enum(Enum { tag, content }));
        }
        match self.entry.value() {
            Value::String(s) => {
                let tag = Tag::new(s, self.entry.span());
                let content = Nothing {
                    span: self.entry.span(),
                };
                self.wrap(visitor.visit_enum(Enum { tag, content }))
            }
            _ => self.wrap(Err(Error::invalid_type(self.unexpected(), &visitor))),
        }
    }
}

// ---------------------------------------------------------------------------
// NodeDe: one node.

/// A single node.
#[derive(Clone, Copy)]
pub(crate) struct NodeDe<'a> {
    n: NodeRef<'a>,
    /// Leading arguments already taken (a map key).
    skip_args: usize,
    /// The node's name is an enum variant (an element of `$children` or a
    /// top-level sequence).
    name_is_variant: bool,
    /// The node's `(type)` annotation was taken by an enum or `Annotated`.
    ty_used: bool,
}

impl<'a> NodeDe<'a> {
    /// A named block's body: the node minus its first argument (the name).
    pub(crate) fn named_body(node: &'a Node<'a>) -> Self {
        Self {
            skip_args: 1,
            ..Self::new(NodeRef { node })
        }
    }

    pub(crate) fn new(n: NodeRef<'a>) -> Self {
        Self {
            n,
            skip_args: 0,
            name_is_variant: false,
            ty_used: false,
        }
    }

    fn args(&self) -> impl Iterator<Item = &'a Entry<'a>> {
        self.n
            .node
            .entries()
            .iter()
            .filter(|e| e.name().is_none())
            .skip(self.skip_args)
    }

    fn props(&self) -> impl Iterator<Item = &'a Entry<'a>> {
        self.n.node.entries().iter().filter(|e| e.name().is_some())
    }

    fn ty(&self) -> Option<&'a kdl_tree::Ident<'a>> {
        if self.ty_used { None } else { self.n.node.ty() }
    }

    fn arg_count(&self) -> usize {
        self.args().count()
    }

    fn has_props(&self) -> bool {
        self.props().next().is_some()
    }

    /// Only positional arguments (at least one), no properties or children.
    fn args_only(&self) -> bool {
        self.arg_count() > 0 && !self.has_props() && !self.n.has_children()
    }

    fn is_empty(&self) -> bool {
        self.arg_count() == 0 && !self.has_props() && !self.n.has_children()
    }

    fn wrap<T>(&self, r: Result<T>) -> Result<T> {
        r.map_err(|e| e.at(self.n.name_span()))
    }

    fn err(&self, kind: ErrorKind, msg: impl Into<String>) -> Error {
        Error::new(kind, msg).at(self.n.name_span())
    }

    /// The node's single argument, for scalar targets.
    fn scalar(&self) -> Result<ValueDe<'a>> {
        let name = self.n.name();
        if let Some(prop) = self.props().next() {
            return Err(Error::new(
                ErrorKind::InvalidType,
                format!("expected a single value for `{name}`, found a property"),
            )
            .with_label("unexpected property")
            .at(prop.span()));
        }
        if self.n.has_children() {
            return Err(self
                .err(
                    ErrorKind::InvalidType,
                    format!("expected a single value for `{name}`, found a children block"),
                )
                .with_label("this node has children"));
        }
        let mut args = self.args();
        let Some(first) = args.next() else {
            return Err(self
                .err(
                    ErrorKind::InvalidType,
                    format!("expected a value for `{name}`, found none"),
                )
                .with_label("missing value")
                .with_help(format!("give it an argument: `{name} <value>`")));
        };
        if let Some(extra) = args.next() {
            return Err(Error::new(
                ErrorKind::InvalidType,
                format!("expected a single value for `{name}`, found several arguments"),
            )
            .with_label("unexpected argument")
            .with_help("only sequence fields (`Vec<T>`, tuples) take several arguments")
            .at(extra.span()));
        }
        Ok(ValueDe::new(first))
    }

    fn fields(&self, fields: &'static [&'static str]) -> Result<Fields<'a>> {
        Fields::of_node(*self, fields)
    }

    /// The node minus what was already taken, as standalone KDL text.
    fn raw_node(&self) -> kdl::KdlNode {
        let mut node = self.n.node.to_kdl();
        let mut skip = self.skip_args;
        node.entries_mut().retain(|e| {
            if e.name().is_none() && skip > 0 {
                skip -= 1;
                false
            } else {
                true
            }
        });
        node
    }
}

macro_rules! node_scalar {
    ($($m:ident)*) => {$(
        fn $m<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
            let v = self.scalar()?;
            v.$m(visitor)
        }
    )*};
}

impl<'a> Deserializer<'a> for NodeDe<'a> {
    type Error = Error;

    node_scalar! {
        deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
        deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
        deserialize_f32 deserialize_f64 deserialize_char deserialize_str deserialize_string
        deserialize_identifier deserialize_bytes deserialize_byte_buf
    }

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        if self.is_empty() {
            self.wrap(visitor.visit_unit())
        } else if self.args_only() && self.arg_count() == 1 {
            self.scalar()?.deserialize_any(visitor)
        } else if self.args_only() {
            self.deserialize_seq(visitor)
        } else {
            self.deserialize_map(visitor)
        }
    }

    fn deserialize_bool<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        // A bare `flag` node is `true`.
        if self.is_empty() {
            return self.wrap(visitor.visit_bool(true));
        }
        self.scalar()?.deserialize_bool(visitor)
    }

    fn deserialize_unit<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        if self.is_empty() {
            return self.wrap(visitor.visit_unit());
        }
        self.scalar()?.deserialize_unit(visitor)
    }

    fn deserialize_unit_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        if self.args_only()
            && self.arg_count() == 1
            && let Some(arg) = self.args().next()
            && *arg.value() == Value::Null
        {
            return self.wrap(visitor.visit_none());
        }
        self.wrap(visitor.visit_some(self))
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        match name {
            magic::RAW_NODE => self.wrap(visitor.visit_string(self.raw_node().to_string())),
            magic::RAW_DOCUMENT => {
                if self.arg_count() > 0 || self.has_props() {
                    return Err(self.err(
                        ErrorKind::InvalidType,
                        "expected only a children block, found arguments or properties",
                    ));
                }
                let doc = self
                    .n
                    .node
                    .children()
                    .map(Document::to_kdl)
                    .unwrap_or_default();
                self.wrap(visitor.visit_string(doc.to_string()))
            }
            _ => self.wrap(visitor.visit_newtype_struct(self)),
        }
    }

    fn deserialize_seq<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        if let Some(prop) = self.props().next() {
            return Err(Error::new(
                ErrorKind::InvalidType,
                format!(
                    "expected a sequence for `{}`, found a property",
                    self.n.name()
                ),
            )
            .with_label("unexpected property")
            .at(prop.span()));
        }
        if self.arg_count() > 0 && self.n.has_children() {
            return Err(self.err(
                ErrorKind::InvalidType,
                format!(
                    "expected a sequence for `{}`: give either arguments or a children block, not both",
                    self.n.name()
                ),
            ));
        }
        let args: Vec<ValueDe<'a>> = self.args().map(ValueDe::new).collect();
        if !args.is_empty() {
            return self.wrap(visitor.visit_seq(ArgSeq {
                args: args.into_iter(),
            }));
        }
        let children: Vec<NodeRef<'a>> = self.n.children().collect();
        self.wrap(visitor.visit_seq(NodeSeq::new(children)))
    }

    fn deserialize_tuple<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let fields = self.fields(&[])?;
        self.wrap(visitor.visit_map(fields))
    }

    fn deserialize_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        if name == magic::ANNOTATED {
            // `size (u8)5`: the annotation is the argument's.
            if self.ty().is_none() && self.args_only() && self.arg_count() == 1 {
                return self.scalar()?.deserialize_struct(name, fields, visitor);
            }
            let ty = self.ty().map(|t| (t.value(), Some(t.span())));
            let value = NodeDe {
                ty_used: true,
                ..self
            };
            return self.wrap(visitor.visit_map(AnnotatedAccess::new(ty, value)));
        }
        let fields = self.fields(fields)?;
        self.wrap(visitor.visit_map(fields))
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        // `(Variant)node …`
        if let Some(ty) = self.ty() {
            let tag = Tag::new(ty.value(), ty.span());
            let content = NodeDe {
                ty_used: true,
                ..self
            };
            return self.wrap(visitor.visit_enum(Enum { tag, content }));
        }
        // `Variant …` inside `$children` or a top-level sequence.
        if self.name_is_variant {
            let tag = Tag::new(self.n.name(), self.n.name_span());
            let content = NodeDe {
                name_is_variant: false,
                ..self
            };
            return self.wrap(visitor.visit_enum(Enum { tag, content }));
        }
        // `node "Variant"` or `node (Variant)value`.
        if self.args_only() && self.arg_count() == 1 {
            return self.scalar()?.deserialize_enum("", &[], visitor);
        }
        // `node { Variant … }`: the children all share one name.
        if self.arg_count() == 0 && !self.has_props() && self.n.has_children() {
            let children: Vec<NodeRef<'a>> = self.n.children().collect();
            if children.iter().all(|c| c.name() == children[0].name()) {
                let first = children[0];
                let tag = Tag::new(first.name(), first.name_span());
                let content = GroupDe { nodes: children };
                return self.wrap(visitor.visit_enum(Enum { tag, content }));
            }
        }
        Err(self
            .err(
                ErrorKind::InvalidType,
                format!("expected an enum variant for `{}`", self.n.name()),
            )
            .with_label("no variant given")
            .with_help(format!(
                "name the variant with a type annotation, `({0}){1} …`, or a value, `{1} \"{0}\"`",
                "Variant",
                self.n.name()
            )))
    }
}

// ---------------------------------------------------------------------------
// GroupDe: the nodes of one name in a block.

/// One or more sibling nodes with the same name: one struct field.
#[derive(Clone)]
pub(crate) struct GroupDe<'a> {
    nodes: Vec<NodeRef<'a>>,
}

impl<'a> GroupDe<'a> {
    /// The single node, or a duplicate-node error.
    fn single(&self) -> Result<NodeDe<'a>> {
        let first = self.nodes[0];
        if let Some(second) = self.nodes.get(1) {
            let name = second.name();
            let mut e = Error::new(
                ErrorKind::DuplicateNode,
                format!("duplicate node `{name}`; it takes a single value"),
            )
            .with_label("given again here")
            .with_help("remove one of them, or make the field a `Vec` to accept several");
            e = e.with_secondary(first.name_span(), "first given here");
            return Err(e.at(second.name_span()));
        }
        Ok(NodeDe::new(first))
    }
}

macro_rules! group_single {
    ($($m:ident)*) => {$(
        fn $m<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
            self.single()?.$m(visitor)
        }
    )*};
}

impl<'a> Deserializer<'a> for GroupDe<'a> {
    type Error = Error;

    group_single! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_identifier deserialize_bytes deserialize_byte_buf
        deserialize_unit
    }

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        if self.nodes.len() > 1 {
            self.deserialize_seq(visitor)
        } else {
            NodeDe::new(self.nodes[0]).deserialize_any(visitor)
        }
    }

    fn deserialize_ignored_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        if self.nodes.len() == 1 {
            return NodeDe::new(self.nodes[0]).deserialize_option(visitor);
        }
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        if magic::is_raw(name) {
            return self.single()?.deserialize_newtype_struct(name, visitor);
        }
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let first = NodeDe::new(self.nodes[0]);
        if self.nodes.len() == 1 {
            // `field` alone is an empty sequence; `field { - a; - b }` lists
            // its elements as `-` children.
            if first.is_empty() {
                return first.wrap(visitor.visit_seq(NodeSeq::new(Vec::new())));
            }
            if first.arg_count() == 0 && !first.has_props() && is_dash_block(first.n) {
                let children = first.n.children().collect();
                return first.wrap(visitor.visit_seq(NodeSeq::new(children)));
            }
        }
        first.wrap(visitor.visit_seq(NodeSeq::new(self.nodes)))
    }

    fn deserialize_tuple<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        // A tuple is one node: `point 1 2`.
        self.single()?.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let first = NodeDe::new(self.nodes[0]);
        // `field { key value; … }` or `field key=value …`: one node, no arguments.
        if self.nodes.len() == 1 && first.arg_count() == 0 {
            return first.deserialize_map(visitor);
        }
        // `field key value` / `field key { … }`, repeated.
        first.wrap(visitor.visit_map(KeyedMap {
            nodes: self.nodes.into_iter(),
            value: None,
        }))
    }

    fn deserialize_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.single()?.deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.single()?.deserialize_enum(name, variants, visitor)
    }
}

// ---------------------------------------------------------------------------
// BlockDe: a document or a children block.

/// A list of sibling nodes: the document (possibly merged from several
/// files) or a node's children.
pub(crate) struct BlockDe<'a> {
    nodes: Vec<NodeRef<'a>>,
    /// Where to point errors about the block as a whole.
    span: RawSpan,
}

impl<'a> BlockDe<'a> {
    pub(crate) fn new(nodes: Vec<NodeRef<'a>>, span: RawSpan) -> Self {
        Self { nodes, span }
    }

    fn wrap<T>(&self, r: Result<T>) -> Result<T> {
        r.map_err(|e| e.with_label_if_unset("in this document").at(self.span))
    }

    fn not_a_value(&self) -> Error {
        Error::new(
            ErrorKind::InvalidType,
            "expected a single value, found a document; deserialize into a struct or map",
        )
        .at(self.span)
    }
}

macro_rules! block_not_value {
    ($($m:ident)*) => {$(
        fn $m<V: Visitor<'a>>(self, _visitor: V) -> Result<V::Value> {
            Err(self.not_a_value())
        }
    )*};
}

impl<'a> Deserializer<'a> for BlockDe<'a> {
    type Error = Error;

    block_not_value! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_identifier deserialize_bytes deserialize_byte_buf
    }

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_map(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    fn deserialize_unit<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        match self.nodes.first() {
            None => visitor.visit_unit(),
            Some(n) => Err(Error::new(
                ErrorKind::InvalidType,
                "expected an empty document, found a node",
            )
            .at(n.name_span())),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        match name {
            magic::RAW_DOCUMENT => {
                let mut doc = kdl::KdlDocument::new();
                doc.nodes_mut()
                    .extend(self.nodes.iter().map(|n| n.node.to_kdl()));
                visitor.visit_string(doc.to_string())
            }
            magic::RAW_NODE => match self.nodes.as_slice() {
                [one] => NodeDe::new(*one).deserialize_newtype_struct(name, visitor),
                _ => Err(Error::new(
                    ErrorKind::InvalidLength,
                    format!("expected exactly one node, found {}", self.nodes.len()),
                )
                .at(self.span)),
            },
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let elements = self
            .nodes
            .iter()
            .map(|n| NodeDe {
                name_is_variant: true,
                ..NodeDe::new(*n)
            })
            .collect::<Vec<_>>();
        self.wrap(visitor.visit_seq(NodeList {
            nodes: elements.into_iter(),
        }))
    }

    fn deserialize_tuple<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let fields = Fields::of_block(&self, &[])?;
        self.wrap(visitor.visit_map(fields))
    }

    fn deserialize_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        let access = Fields::of_block(&self, fields)?;
        self.wrap(visitor.visit_map(access))
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        // A document holding one enum value: the single node names the variant.
        match self.nodes.as_slice() {
            [one] => {
                let tag = Tag::new(one.name(), one.name_span());
                visitor.visit_enum(Enum {
                    tag,
                    content: NodeDe::new(*one),
                })
            }
            _ => Err(Error::new(
                ErrorKind::InvalidType,
                format!(
                    "expected one node naming an enum variant, found {} nodes",
                    self.nodes.len()
                ),
            )
            .at(self.span)),
        }
    }
}

// ---------------------------------------------------------------------------
// Struct and map access.

/// A key: a field name, property name, node name or `$N`.
struct Key<'a> {
    name: Cow<'a, str>,
    span: RawSpan,
    /// The key is a property, so errors say "property", not "field".
    prop: bool,
}

/// What a key's value deserializes from.
enum Val<'a> {
    Value(ValueDe<'a>),
    Group(GroupDe<'a>),
    Args(Vec<ValueDe<'a>>),
    Str(StrDe<'a>),
    Block(BlockDe<'a>),
}

/// The keys and values of a node or block, as a struct or map sees them.
struct Fields<'a> {
    entries: alloc::vec::IntoIter<(Key<'a>, Val<'a>)>,
    value: Option<Val<'a>>,
    /// Where the current value is, for errors its `Deserialize` impl raises.
    value_span: RawSpan,
}

impl<'a> Fields<'a> {
    fn of_node(de: NodeDe<'a>, fields: &'static [&'static str]) -> Result<Self> {
        let n = de.n;
        let mut out: Vec<(Key<'a>, Val<'a>)> = Vec::new();
        let has = |f: &str| fields.contains(&f);

        // Positional arguments: `$0`, `$1`, …; the rest go to `$args`.
        let mut rest = Vec::new();
        for (i, arg) in de.args().enumerate() {
            let name = format!("${i}");
            let value = ValueDe::new(arg);
            if has(&name) || !has("$args") {
                let key = Key {
                    name: Cow::Owned(name),
                    span: arg.span(),
                    prop: false,
                };
                out.push((key, Val::Value(value)));
            } else {
                rest.push(value);
            }
        }
        if has("$args") {
            let key = Key {
                name: Cow::Borrowed("$args"),
                span: n.name_span(),
                prop: false,
            };
            out.push((key, Val::Args(rest)));
        }
        if has("$name") {
            let key = Key {
                name: Cow::Borrowed("$name"),
                span: n.name_span(),
                prop: false,
            };
            out.push((key, Val::Str(StrDe::new(n.name(), n.name_span()))));
        }
        if let (true, Some(ty)) = (has("$type"), de.ty()) {
            let key = Key {
                name: Cow::Borrowed("$type"),
                span: ty.span(),
                prop: false,
            };
            out.push((key, Val::Str(StrDe::new(ty.value(), ty.span()))));
        }

        // Properties: `@key` fields, or plain fields. The last one wins, as
        // the KDL spec says.
        let props: Vec<&Entry<'a>> = de.props().collect();
        let mut prop_names: Vec<&'a str> = Vec::new();
        for (i, prop) in props.iter().enumerate() {
            let key = prop.name().map_or("", |k| k.value());
            if props[i + 1..]
                .iter()
                .any(|p| p.name().map(kdl_tree::Ident::value) == Some(key))
            {
                continue;
            }
            prop_names.push(key);
            let at = format!("@{key}");
            let name = if has(&at) {
                Cow::Owned(at)
            } else {
                Cow::Borrowed(key)
            };
            let k = Key {
                name,
                span: prop.span(),
                prop: true,
            };
            out.push((k, Val::Value(ValueDe::new(prop))));
        }

        let children: Vec<NodeRef<'a>> = n.children().collect();
        Self::push_children(&mut out, &children, fields, &prop_names)?;
        let span = n
            .node
            .children()
            .map_or(n.name_span(), kdl_tree::Document::span);
        Self::push_rest(&mut out, children, fields, span);
        Ok(Self {
            entries: out.into_iter(),
            value: None,
            value_span: RawSpan::default(),
        })
    }

    fn of_block(block: &BlockDe<'a>, fields: &'static [&'static str]) -> Result<Self> {
        let mut out = Vec::new();
        Self::push_children(&mut out, &block.nodes, fields, &[])?;
        Self::push_rest(&mut out, block.nodes.clone(), fields, block.span);
        Ok(Self {
            entries: out.into_iter(),
            value: None,
            value_span: RawSpan::default(),
        })
    }

    /// Children grouped by name, in order of first appearance.
    fn push_children(
        out: &mut Vec<(Key<'a>, Val<'a>)>,
        children: &[NodeRef<'a>],
        fields: &'static [&'static str],
        prop_names: &[&'a str],
    ) -> Result<()> {
        let rest = fields.contains(&"$children");
        let mut groups: Vec<(&'a str, Vec<NodeRef<'a>>)> = Vec::new();
        // Index groups by name so wide blocks stay linear.
        let mut index: std::collections::HashMap<&'a str, usize> =
            std::collections::HashMap::with_capacity(children.len().min(64));
        for c in children {
            let name = c.name();
            if rest && !fields.contains(&name) {
                continue;
            }
            if let Some(&i) = index.get(name) {
                groups[i].1.push(*c);
            } else {
                index.insert(name, groups.len());
                groups.push((name, vec![*c]));
            }
        }
        for (name, nodes) in groups {
            let first = nodes[0];
            if prop_names.contains(&name) {
                return Err(Error::new(
                    ErrorKind::DuplicateField,
                    format!("`{name}` is given both as a property and as a child node"),
                )
                .with_label("given again here")
                .at(first.name_span()));
            }
            let key = Key {
                name: Cow::Borrowed(name),
                span: first.name_span(),
                prop: false,
            };
            out.push((key, Val::Group(GroupDe { nodes })));
        }
        Ok(())
    }

    /// `$children`: the children no other field took.
    fn push_rest(
        out: &mut Vec<(Key<'a>, Val<'a>)>,
        children: Vec<NodeRef<'a>>,
        fields: &'static [&'static str],
        span: RawSpan,
    ) {
        if !fields.contains(&"$children") {
            return;
        }
        let rest = children
            .into_iter()
            .filter(|c| !fields.contains(&c.name()))
            .collect();
        let key = Key {
            name: Cow::Borrowed("$children"),
            span,
            prop: false,
        };
        out.push((key, Val::Block(BlockDe::new(rest, span))));
    }
}

impl<'a> MapAccess<'a> for Fields<'a> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'a>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        self.value_span = key.span;
        let prop = key.prop;
        let de = StrDe::new_cow(key.name, key.span);
        seed.deserialize(de)
            .map(Some)
            .map_err(|e| if prop { e.for_property() } else { e })
    }

    fn next_value_seed<V: DeserializeSeed<'a>>(&mut self, seed: V) -> Result<V::Value> {
        let r = match self.value.take() {
            Some(Val::Value(v)) => seed.deserialize(v),
            Some(Val::Group(g)) => seed.deserialize(g),
            Some(Val::Args(args)) => seed.deserialize(ArgsDe(args)),
            Some(Val::Str(s)) => seed.deserialize(s),
            Some(Val::Block(b)) => seed.deserialize(b),
            None => Err(Error::custom("internal: value requested before key")),
        };
        r.map_err(|e| e.at(self.value_span))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// `field key value` / `field key { … }` nodes as map entries.
struct KeyedMap<'a> {
    nodes: alloc::vec::IntoIter<NodeRef<'a>>,
    value: Option<NodeDe<'a>>,
}

impl<'a> MapAccess<'a> for KeyedMap<'a> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'a>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        let Some(n) = self.nodes.next() else {
            return Ok(None);
        };
        let de = NodeDe::new(n);
        let Some(key) = de.args().next() else {
            return Err(de
                .err(
                    ErrorKind::InvalidType,
                    format!("expected a key after `{}`", n.name()),
                )
                .with_label("missing key")
                .with_help(format!(
                    "write map entries as `{0} <key> <value>` or `{0} <key> {{ … }}`",
                    n.name()
                )));
        };
        self.value = Some(NodeDe { skip_args: 1, ..de });
        seed.deserialize(ValueDe::new(key)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'a>>(&mut self, seed: V) -> Result<V::Value> {
        match self.value.take() {
            Some(v) => v.wrap(seed.deserialize(v)),
            None => Err(Error::custom("internal: value requested before key")),
        }
    }
}

// ---------------------------------------------------------------------------
// Sequences.

/// Values (positional arguments) as a sequence.
struct ArgSeq<'a> {
    args: alloc::vec::IntoIter<ValueDe<'a>>,
}

impl<'a> SeqAccess<'a> for ArgSeq<'a> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'a>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        self.args
            .next()
            .map(|v| v.wrap(seed.deserialize(v)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.args.len())
    }
}

/// `$args`: the positional arguments no `$N` field took.
struct ArgsDe<'a>(Vec<ValueDe<'a>>);

impl<'a> Deserializer<'a> for ArgsDe<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_seq(ArgSeq {
            args: self.0.into_iter(),
        })
    }

    serde::forward_to_deserialize_any! {
        <W: Visitor<'a>>
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// Whole nodes as a sequence (top-level documents, `$children`).
struct NodeList<'a> {
    nodes: alloc::vec::IntoIter<NodeDe<'a>>,
}

impl<'a> SeqAccess<'a> for NodeList<'a> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'a>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        self.nodes
            .next()
            .map(|n| n.wrap(seed.deserialize(n)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.nodes.len())
    }
}

/// Repeated nodes as a sequence. Each node is one element (`member { … }`)
/// or, when the element type is a scalar, contributes all its arguments
/// (`tags "a" "b"`); the element type decides, via [`ElemDe`].
struct NodeSeq<'a> {
    nodes: alloc::vec::IntoIter<NodeRef<'a>>,
    /// Remaining arguments of a node being read argument by argument.
    args: alloc::vec::IntoIter<ValueDe<'a>>,
}

impl<'a> NodeSeq<'a> {
    fn new(nodes: Vec<NodeRef<'a>>) -> Self {
        Self {
            nodes: nodes.into_iter(),
            args: Vec::new().into_iter(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Undecided,
    Args,
    Whole,
}

impl<'a> SeqAccess<'a> for NodeSeq<'a> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'a>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if let Some(v) = self.args.next() {
            return v.wrap(seed.deserialize(v)).map(Some);
        }
        let Some(n) = self.nodes.next() else {
            return Ok(None);
        };
        let mut mode = Mode::Undecided;
        let de = NodeDe::new(n);
        let value = de.wrap(seed.deserialize(ElemDe {
            de,
            mode: &mut mode,
        }))?;
        if mode == Mode::Args {
            let rest: Vec<ValueDe<'a>> = NodeDe::new(n).args().skip(1).map(ValueDe::new).collect();
            self.args = rest.into_iter();
        }
        Ok(Some(value))
    }
}

/// One node in a [`NodeSeq`], not yet known to be one element or a list of
/// argument elements.
struct ElemDe<'a, 'm> {
    de: NodeDe<'a>,
    mode: &'m mut Mode,
}

impl<'a> ElemDe<'a, '_> {
    /// Read argument by argument when the node is only arguments.
    fn by_args(&mut self) -> Option<ValueDe<'a>> {
        if self.de.args_only() && self.de.ty().is_none() {
            *self.mode = Mode::Args;
            self.de.args().next().map(ValueDe::new)
        } else {
            *self.mode = Mode::Whole;
            None
        }
    }

    fn whole(self) -> NodeDe<'a> {
        *self.mode = Mode::Whole;
        self.de
    }
}

macro_rules! elem_scalar {
    ($($m:ident)*) => {$(
        fn $m<V: Visitor<'a>>(mut self, visitor: V) -> Result<V::Value> {
            match self.by_args() {
                Some(v) => v.$m(visitor),
                None => self.de.$m(visitor),
            }
        }
    )*};
}

macro_rules! elem_whole {
    ($($m:ident)*) => {$(
        fn $m<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
            self.whole().$m(visitor)
        }
    )*};
}

impl<'a> Deserializer<'a> for ElemDe<'a, '_> {
    type Error = Error;

    elem_scalar! {
        deserialize_any deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32
        deserialize_i64 deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32
        deserialize_u64 deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char
        deserialize_str deserialize_string deserialize_identifier deserialize_bytes
        deserialize_byte_buf deserialize_option deserialize_unit
    }

    elem_whole! {
        deserialize_seq deserialize_map deserialize_ignored_any
    }

    fn deserialize_unit_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        if magic::is_raw(name) {
            return self.whole().deserialize_newtype_struct(name, visitor);
        }
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_tuple<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.whole().deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        self.whole().deserialize_seq(visitor)
    }

    fn deserialize_struct<V: Visitor<'a>>(
        mut self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        if name == magic::ANNOTATED
            && let Some(v) = self.by_args()
        {
            return v.deserialize_struct(name, fields, visitor);
        }
        self.whole().deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'a>>(
        mut self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        match self.by_args() {
            Some(v) => v.deserialize_enum(name, variants, visitor),
            None => self.de.deserialize_enum(name, variants, visitor),
        }
    }
}

// ---------------------------------------------------------------------------
// Small deserializers: strings, nothing.

/// A string with a location: keys, node names, type annotations.
pub(crate) struct StrDe<'a> {
    s: Cow<'a, str>,
    span: RawSpan,
}

impl<'a> StrDe<'a> {
    fn new(s: &'a str, span: RawSpan) -> Self {
        Self::new_cow(Cow::Borrowed(s), span)
    }

    fn new_cow(s: Cow<'a, str>, span: RawSpan) -> Self {
        Self { s, span }
    }

    fn parse<T: core::str::FromStr>(&self, exp: &dyn de::Expected) -> Result<T> {
        self.s
            .parse()
            .map_err(|_| Error::invalid_value(Unexpected::Str(&self.s), exp))
            .map_err(|e: Error| e.at(self.span))
    }
}

macro_rules! str_parse {
    ($($m:ident $t:ty => $visit:ident,)*) => {$(
        fn $m<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
            let v: $t = self.parse(&visitor)?;
            visitor.$visit(v).map_err(|e: Error| e.at(self.span))
        }
    )*};
}

impl<'a> Deserializer<'a> for StrDe<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        let r = match self.s {
            Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
            Cow::Owned(s) => visitor.visit_string(s),
        };
        r.map_err(|e: Error| e.at(self.span))
    }

    // Map keys from node names may be numbers or booleans: `ports { 80 "http" }`.
    str_parse! {
        deserialize_bool bool => visit_bool,
        deserialize_i8 i8 => visit_i8,
        deserialize_i16 i16 => visit_i16,
        deserialize_i32 i32 => visit_i32,
        deserialize_i64 i64 => visit_i64,
        deserialize_i128 i128 => visit_i128,
        deserialize_u8 u8 => visit_u8,
        deserialize_u16 u16 => visit_u16,
        deserialize_u32 u32 => visit_u32,
        deserialize_u64 u64 => visit_u64,
        deserialize_u128 u128 => visit_u128,
        deserialize_f32 f32 => visit_f32,
        deserialize_f64 f64 => visit_f64,
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        let span = self.span;
        let content = Nothing { span };
        visitor
            .visit_enum(Enum {
                tag: Tag { name: self.s, span },
                content,
            })
            .map_err(|e: Error| e.at(span))
    }

    serde::forward_to_deserialize_any! {
        <W: Visitor<'a>>
        char str string bytes byte_buf unit unit_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

/// The content of a unit variant given as a bare string (`mode "fast"`).
struct Nothing {
    span: RawSpan,
}

impl<'a> Deserializer<'a> for Nothing {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit().map_err(|e: Error| {
            e.with_help("non-unit variants are written `(Variant)value` or `(Variant)node { … }`")
                .at(self.span)
        })
    }

    serde::forward_to_deserialize_any! {
        <W: Visitor<'a>>
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

// ---------------------------------------------------------------------------
// Enums.

/// A variant name and where it was written.
struct Tag<'a> {
    name: Cow<'a, str>,
    span: RawSpan,
}

impl<'a> Tag<'a> {
    fn new(name: &'a str, span: RawSpan) -> Self {
        Self {
            name: Cow::Borrowed(name),
            span,
        }
    }
}

struct Enum<'a, D> {
    tag: Tag<'a>,
    content: D,
}

impl<'a, D: VariantAccess<'a, Error = Error>> EnumAccess<'a> for Enum<'a, D> {
    type Error = Error;
    type Variant = D;

    fn variant_seed<S: DeserializeSeed<'a>>(self, seed: S) -> Result<(S::Value, D)> {
        let Tag { name, span } = self.tag;
        let label = format!("`{name}` is not a variant");
        let v = seed
            .deserialize(name.into_deserializer())
            .map_err(|e: Error| {
                let e = if e.has_span() { e } else { e.with_label(label) };
                e.at(span)
            })?;
        Ok((v, self.content))
    }
}

impl<'a> VariantAccess<'a> for NodeDe<'a> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        de::Deserialize::deserialize(self)
    }

    fn newtype_variant_seed<T: DeserializeSeed<'a>>(self, seed: T) -> Result<T::Value> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn struct_variant<V: Visitor<'a>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_struct("", fields, visitor)
    }
}

impl<'a> VariantAccess<'a> for GroupDe<'a> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        de::Deserialize::deserialize(self)
    }

    fn newtype_variant_seed<T: DeserializeSeed<'a>>(self, seed: T) -> Result<T::Value> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'a>>(self, len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V: Visitor<'a>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_struct("", fields, visitor)
    }
}

impl<'a> VariantAccess<'a> for ValueDe<'a> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        de::Deserialize::deserialize(self)
    }

    fn newtype_variant_seed<T: DeserializeSeed<'a>>(self, seed: T) -> Result<T::Value> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn struct_variant<V: Visitor<'a>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_seq(visitor)
    }
}

impl<'a> VariantAccess<'a> for Nothing {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'a>>(self, seed: T) -> Result<T::Value> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'a>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        self.deserialize_any(visitor)
    }

    fn struct_variant<V: Visitor<'a>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_any(visitor)
    }
}

// ---------------------------------------------------------------------------
// Annotated<T>.

/// `$type` (the annotation) then `$value` (the value without it).
struct AnnotatedAccess<'a, D> {
    ty: Option<(&'a str, Option<RawSpan>)>,
    value: Option<D>,
    step: u8,
}

impl<'a, D> AnnotatedAccess<'a, D> {
    fn new(ty: Option<(&'a str, Option<RawSpan>)>, value: D) -> Self {
        Self {
            ty,
            value: Some(value),
            step: 0,
        }
    }
}

impl<'a, D: Deserializer<'a, Error = Error>> MapAccess<'a> for AnnotatedAccess<'a, D> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'a>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        let key = match self.step {
            0 => magic::ANNOTATED_TYPE,
            1 => magic::ANNOTATED_VALUE,
            _ => return Ok(None),
        };
        seed.deserialize(key.into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'a>>(&mut self, seed: V) -> Result<V::Value> {
        self.step += 1;
        if self.step == 1 {
            return match self.ty {
                Some((ty, span)) => {
                    seed.deserialize(OptStr(Some(StrDe::new(ty, span.unwrap_or_default()))))
                }
                None => seed.deserialize(OptStr(None)),
            };
        }
        match self.value.take() {
            Some(v) => seed.deserialize(v),
            None => Err(Error::custom("internal: annotated value read twice")),
        }
    }
}

struct OptStr<'a>(Option<StrDe<'a>>);

impl<'a> Deserializer<'a> for OptStr<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value> {
        match self.0 {
            Some(s) => visitor.visit_some(s),
            None => visitor.visit_none(),
        }
    }

    serde::forward_to_deserialize_any! {
        <W: Visitor<'a>>
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}
