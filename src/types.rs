//! Helper types: [`Annotated`], [`RawNode`] and [`RawDocument`].

use core::fmt;

use kdl::{KdlDocument, KdlNode};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::magic;

/// A value and its KDL type annotation: `(u8)5`, `(date)"2026-01-01"`,
/// `(point)pos 1 2`.
///
/// On a scalar field the annotation is the value's; on a node-shaped value
/// (a struct) it is the node's. A missing annotation reads as `None`.
///
/// ```
/// use kdl_serde::Annotated;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Event {
///     at: Annotated<String>,
/// }
///
/// let ev: Event = kdl_serde::from_str(r#"at (date)"2026-11-12""#).unwrap();
/// assert_eq!(ev.at.ty.as_deref(), Some("date"));
/// assert_eq!(ev.at.value, "2026-11-12");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Annotated<T> {
    /// The annotation, without parentheses.
    pub ty: Option<String>,
    /// The annotated value.
    pub value: T,
}

impl<T> Annotated<T> {
    /// A value with annotation `ty`.
    pub fn new(ty: impl Into<String>, value: T) -> Self {
        Self {
            ty: Some(ty.into()),
            value,
        }
    }

    /// A value without an annotation.
    pub fn plain(value: T) -> Self {
        Self { ty: None, value }
    }
}

impl<T: Serialize> Serialize for Annotated<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct(magic::ANNOTATED, 2)?;
        s.serialize_field(magic::ANNOTATED_TYPE, &self.ty)?;
        s.serialize_field(magic::ANNOTATED_VALUE, &self.value)?;
        s.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Annotated<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<T>(core::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Annotated<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an annotated value")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut ty = None;
                let mut value = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        magic::ANNOTATED_TYPE => ty = map.next_value()?,
                        magic::ANNOTATED_VALUE => value = Some(map.next_value()?),
                        _ => {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }
                let value =
                    value.ok_or_else(|| de::Error::missing_field(magic::ANNOTATED_VALUE))?;
                Ok(Annotated { ty, value })
            }
        }

        deserializer.deserialize_struct(
            magic::ANNOTATED,
            magic::ANNOTATED_FIELDS,
            V(core::marker::PhantomData),
        )
    }
}

macro_rules! raw_type {
    ($(#[$doc:meta])* $name:ident, $inner:ty, $magic:expr, $what:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(pub $inner);

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_newtype_struct($magic, &self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct V;

                impl<'de> Visitor<'de> for V {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str($what)
                    }

                    fn visit_str<E: de::Error>(self, s: &str) -> Result<$name, E> {
                        s.parse().map($name).map_err(|e: kdl::KdlError| {
                            let msg = e
                                .diagnostics
                                .first()
                                .and_then(|d| d.message.clone())
                                .unwrap_or_else(|| "invalid KDL".into());
                            E::custom(format!("invalid KDL: {msg}"))
                        })
                    }

                    fn visit_newtype_struct<D: Deserializer<'de>>(
                        self,
                        d: D,
                    ) -> Result<$name, D::Error> {
                        d.deserialize_str(self)
                    }
                }

                deserializer.deserialize_newtype_struct($magic, V)
            }
        }
    };
}

raw_type! {
    /// A node passed through untouched: its arguments, properties, children
    /// and annotations, as a [`KdlNode`]. Formatting and spans are not kept.
    ///
    /// ```
    /// use kdl_serde::RawNode;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Plugin {
    ///     name: String,
    ///     config: RawNode,
    /// }
    ///
    /// let p: Plugin = kdl_serde::from_str(r#"
    ///     name "lint"
    ///     config level=2 { rule "a" }
    /// "#).unwrap();
    /// assert_eq!(p.config.0.get("level"), Some(&kdl::KdlValue::Integer(2)));
    /// ```
    RawNode, KdlNode, magic::RAW_NODE, "a KDL node"
}

raw_type! {
    /// A children block (or a whole document) passed through untouched as a
    /// [`KdlDocument`]. Formatting and spans are not kept.
    ///
    /// ```
    /// use kdl_serde::RawDocument;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Plugin {
    ///     name: String,
    ///     settings: RawDocument,
    /// }
    ///
    /// let p: Plugin = kdl_serde::from_str(r#"
    ///     name "lint"
    ///     settings { level 2; rules "a" "b" }
    /// "#).unwrap();
    /// assert_eq!(p.settings.0.nodes().len(), 2);
    /// ```
    RawDocument, KdlDocument, magic::RAW_DOCUMENT, "a KDL document"
}
