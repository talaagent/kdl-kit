#![doc = include_str!("../README.md")]

extern crate alloc;

mod base64;
mod de;
mod error;
mod magic;
mod ser;
mod types;

use alloc::sync::Arc;
use std::io::Read;
use std::path::{Path, PathBuf};

use kdl::KdlDocument;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use crate::error::{Error, ErrorKind, Result};
pub use crate::ser::{MapStyle, Options, SeqStyle};
pub use crate::types::{Annotated, RawDocument, RawNode};

use crate::de::{BlockDe, NodeRef};
use crate::error::Source;

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
    from_source(Source::new(None, s))
}

/// Deserializes a `T` from KDL text read from a file named `name`; errors
/// name the file.
pub fn from_str_named<T: DeserializeOwned>(s: &str, name: impl Into<String>) -> Result<T> {
    from_source(Source::new(Some(name.into()), s))
}

/// Deserializes a `T` from a reader holding KDL text.
pub fn from_reader<T: DeserializeOwned, R: Read>(mut reader: R) -> Result<T> {
    let mut s = String::new();
    reader
        .read_to_string(&mut s)
        .map_err(|e| Error::io(PathBuf::from("<reader>"), e))?;
    from_str(&s)
}

/// Deserializes a `T` from a parsed document.
///
/// Error spans point into the document's own text (its `Display` output),
/// which matches the original input when the document came from
/// [`KdlDocument::parse`] and has not been edited.
pub fn from_doc<T: DeserializeOwned>(doc: &KdlDocument) -> Result<T> {
    let src = Source::new(None, doc.to_string());
    deserialize(&[(doc, &src)])
}

/// Deserializes a `T` from a KDL file.
pub fn from_file<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path.to_owned(), e))?;
    from_source(Source::new(Some(path.display().to_string()), text))
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
    let mut files = Vec::new();
    collect_kdl_files(dir.as_ref(), recursive, &mut files)?;
    files.sort();
    let mut sources = Vec::with_capacity(files.len());
    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.clone(), e))?;
        sources.push(Source::new(Some(path.display().to_string()), text));
    }
    let docs = sources
        .iter()
        .map(|src| parse(src).map(|doc| (doc, src)))
        .collect::<Result<Vec<_>>>()?;
    let refs: Vec<(&KdlDocument, &Arc<Source>)> = docs.iter().map(|(d, s)| (d, *s)).collect();
    deserialize(&refs)
}

fn collect_kdl_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir.to_owned(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir.to_owned(), e))?;
        let path = entry.path();
        let ty = entry.file_type().map_err(|e| Error::io(path.clone(), e))?;
        if ty.is_dir() {
            if recursive {
                collect_kdl_files(&path, recursive, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "kdl") {
            out.push(path);
        }
    }
    Ok(())
}

fn parse(src: &Arc<Source>) -> Result<KdlDocument> {
    KdlDocument::parse_v2(&src.text).map_err(|e| Error::parse(src, &e))
}

fn from_source<T: DeserializeOwned>(src: Arc<Source>) -> Result<T> {
    let doc = parse(&src)?;
    deserialize(&[(&doc, &src)])
}

fn deserialize<T: DeserializeOwned>(docs: &[(&KdlDocument, &Arc<Source>)]) -> Result<T> {
    let nodes: Vec<NodeRef<'_>> = docs
        .iter()
        .flat_map(|(doc, src)| doc.nodes().iter().map(move |node| NodeRef { node, src }))
        .collect();
    let empty = Source::new(None, "");
    let src = docs.first().map_or(&empty, |(_, s)| *s);
    T::deserialize(BlockDe::new(nodes, src, (0, 0).into()))
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
