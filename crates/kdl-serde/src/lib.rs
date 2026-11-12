#![doc = include_str!("../README.md")]

extern crate alloc;

mod base64;
mod de;
mod error;
mod magic;
mod ser;
mod types;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use kdl::KdlDocument;
pub use kdl_load::Loaded;
use kdl_span::SourceMap;
use kdl_tree::Document;
use serde::Serialize;
use serde::de::{Deserialize, DeserializeOwned};

pub use crate::error::{Error, ErrorKind, Result};
pub use crate::ser::{MapStyle, Options, SeqStyle};
pub use crate::types::{Annotated, RawDocument, RawNode};

use crate::de::{BlockDe, NodeDe, NodeRef};

/// Deserializes a `T` from KDL text.
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Server {
///     host: String,
///     port: u16,
/// }
///
/// let s: Server = kdl_serde::from_str("host \"localhost\"\nport 8080").unwrap();
/// assert_eq!((s.host.as_str(), s.port), ("localhost", 8080));
/// ```
pub fn from_str<T: DeserializeOwned>(s: &str) -> Result<T> {
    from_str_named(s, "")
}

/// Deserializes a `T` from KDL text that came from a file named `name`;
/// errors name the file.
pub fn from_str_named<T: DeserializeOwned>(s: &str, name: &str) -> Result<T> {
    // Parse borrowing from `s`; build a source map only for an error.
    let sources = || {
        let mut sources = SourceMap::new();
        sources.add(name, s);
        sources
    };
    match Document::parse(s) {
        Ok(doc) => T::deserialize(document(&doc)).map_err(|e| e.resolve(&sources())),
        Err(_) => match kdl_load::load_str(name, s) {
            Err(e) => Err(Error::load(e)),
            Ok(loaded) => from_loaded(&loaded),
        },
    }
}

/// Deserializes a `T` from a reader holding KDL text.
pub fn from_reader<T: DeserializeOwned, R: Read>(mut reader: R) -> Result<T> {
    let mut s = String::new();
    reader.read_to_string(&mut s).map_err(|source| {
        Error::load(kdl_load::Error::Io {
            path: "<reader>".into(),
            source,
        })
    })?;
    from_str(&s)
}

/// Deserializes a `T` from a KDL file.
pub fn from_file<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    from_loaded(&kdl_load::load_file(path).map_err(Error::load)?)
}

/// Deserializes a `T` from every `*.kdl` file in `dir` (and its
/// subdirectories when `recursive`), read as one document: the files' nodes
/// in order, files sorted by path. Errors name the file they come from.
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let dir = tempfile::tempdir()?;
/// # std::fs::write(dir.path().join("a.kdl"), "member \"ada\"")?;
/// # std::fs::write(dir.path().join("b.kdl"), "member \"grace\"")?;
/// #[derive(serde::Deserialize)]
/// struct Team {
///     member: Vec<String>,
/// }
///
/// let team: Team = kdl_serde::from_dir(dir.path(), false)?;
/// assert_eq!(team.member, ["ada", "grace"]);
/// # Ok(())
/// # }
/// ```
pub fn from_dir<T: DeserializeOwned>(dir: impl AsRef<Path>, recursive: bool) -> Result<T> {
    from_loaded(&kdl_load::load_dir(dir, recursive).map_err(Error::load)?)
}

/// Deserializes a `T` from a document loaded with `kdl-load`. `T` may
/// borrow strings from the document (`&'de str`, `Cow<'de, str>`).
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Server<'a> {
///     host: &'a str,
/// }
///
/// let loaded = kdl_load::load_str("", "host \"localhost\"").unwrap();
/// let s: Server<'_> = kdl_serde::from_loaded(&loaded).unwrap();
/// assert_eq!(s.host, "localhost");
/// ```
pub fn from_loaded<'de, T: Deserialize<'de>>(loaded: &'de Loaded) -> Result<T> {
    from_tree(loaded.tree(), loaded.sources())
}

/// Deserializes a `T` from a `kdl-tree` document whose spans resolve
/// against `sources`; `T` may borrow from it.
///
/// ```
/// let text = "host \"localhost\"";
/// let doc = kdl_tree::Document::parse(text).unwrap();
/// let mut sources = kdl_span::SourceMap::new();
/// sources.add("config.kdl", text);
/// let host: std::collections::BTreeMap<&str, &str> = kdl_serde::from_tree(&doc, &sources).unwrap();
/// assert_eq!(host["host"], "localhost");
/// ```
pub fn from_tree<'de, T: Deserialize<'de>>(
    doc: &'de Document<'de>,
    sources: &SourceMap,
) -> Result<T> {
    T::deserialize(document(doc)).map_err(|e| e.resolve(sources))
}

/// Deserializes a `T` from a `kdl` document.
///
/// Error spans point into the document's own text (its `Display` output),
/// which matches the original input when the document came from
/// [`KdlDocument::parse`] and has not been edited.
#[cfg(feature = "kdl-compat")]
#[deprecated(
    note = "use `from_str`, `from_loaded` or `from_tree`; `kdl` input goes away with \
                     the `kdl-compat` feature"
)]
pub fn from_doc<T: DeserializeOwned>(doc: &KdlDocument) -> Result<T> {
    let tree = Document::from_kdl(doc);
    T::deserialize(document(&tree)).map_err(|e| {
        let mut sources = SourceMap::new();
        sources.add("", doc.to_string());
        e.resolve(&sources)
    })
}

fn document<'a>(doc: &'a Document<'a>) -> BlockDe<'a> {
    let nodes: Vec<NodeRef<'a>> = doc.nodes().iter().map(|node| NodeRef { node }).collect();
    BlockDe::new(nodes, kdl_tree::Span::default())
}

/// Deserializes every `<kind> <name> { body }` node of a loaded document:
/// the name is the node's single string argument, the body (properties,
/// further arguments and children) deserializes as `T`. Names must be
/// unique (`kdl_nodes::duplicate_name`).
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Struct {
///     field: Vec<String>,
/// }
///
/// let loaded = kdl_load::load_str("", "struct User { field id }\nstruct Post { field title }").unwrap();
/// let structs = kdl_serde::named_blocks::<Struct>(&loaded, "struct").unwrap();
/// assert_eq!(structs[1].0, "Post");
/// assert_eq!(structs[1].1.field, ["title"]);
/// ```
pub fn named_blocks<'de, T: Deserialize<'de>>(
    loaded: &'de Loaded,
    kind: &str,
) -> Result<Vec<(&'de str, T)>> {
    let sources = loaded.sources();
    let blocks = kdl_nodes::named_blocks(loaded.tree(), kind, sources)
        .map_err(|e| Error::nodes(e, sources))?;
    blocks
        .into_iter()
        .map(|b| {
            let value =
                T::deserialize(NodeDe::named_body(b.node)).map_err(|e| e.resolve(sources))?;
            Ok((b.name, value))
        })
        .collect()
}

/// [`named_blocks`] as a map from name to value.
pub fn named_map<'de, T: Deserialize<'de>>(
    loaded: &'de Loaded,
    kind: &str,
) -> Result<BTreeMap<String, T>> {
    Ok(named_blocks(loaded, kind)?
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect())
}

/// Serializes `value` to KDL text, one top-level node per line with
/// children inline (`a { b 1; c 2; }`). See [`to_string_pretty`] for
/// indented output.
pub fn to_string<T: ?Sized + Serialize>(value: &T) -> Result<String> {
    Ok(kdl_pretty::format_compact(&to_doc(value)?))
}

/// Serializes `value` to indented KDL text.
///
/// ```
/// #[derive(serde::Serialize)]
/// struct Server {
///     host: String,
///     tags: Vec<String>,
/// }
///
/// let s = Server { host: "localhost".into(), tags: vec!["a".into(), "b".into()] };
/// assert_eq!(kdl_serde::to_string_pretty(&s).unwrap(), "host \"localhost\"\ntags \"a\" \"b\"\n");
/// ```
pub fn to_string_pretty<T: ?Sized + Serialize>(value: &T) -> Result<String> {
    to_string_with(value, &Options::default())
}

/// Serializes `value` to indented KDL text with `opts`.
pub fn to_string_with<T: ?Sized + Serialize>(value: &T, opts: &Options) -> Result<String> {
    Ok(kdl_pretty::format(
        &to_doc_with(value, opts)?,
        &opts.pretty(),
    ))
}

/// Serializes `value` to a [`KdlDocument`].
pub fn to_doc<T: ?Sized + Serialize>(value: &T) -> Result<KdlDocument> {
    to_doc_with(value, &Options::default())
}

/// Serializes `value` to a [`KdlDocument`] with `opts`.
pub fn to_doc_with<T: ?Sized + Serialize>(value: &T, opts: &Options) -> Result<KdlDocument> {
    ser::to_document(value, opts)
}
