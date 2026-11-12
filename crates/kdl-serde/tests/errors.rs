//! Snapshots of rendered error diagnostics.

#![cfg(feature = "miette")]
#![allow(clippy::unwrap_used, dead_code)]

use std::collections::BTreeMap;

use kdl_serde::ErrorKind;
use miette::{GraphicalReportHandler, GraphicalTheme};
use serde::Deserialize;

fn render(e: &kdl_serde::Error) -> String {
    render_width(e, 100)
}

/// Wide enough that temporary paths never wrap.
fn render_width(e: &kdl_serde::Error, width: usize) -> String {
    let mut out = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
        .with_width(width)
        .with_links(false)
        .render_report(&mut out, e)
        .unwrap();
    out
}

fn fail<T: serde::de::DeserializeOwned + core::fmt::Debug>(src: &str) -> kdl_serde::Error {
    kdl_serde::from_str_named::<T>(src, "config.kdl").unwrap_err()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Server {
    host: String,
    port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    name: String,
    server: Server,
    #[serde(default)]
    member: Vec<Member>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "@role")]
    role: Role,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    Admin,
    Viewer,
}

#[test]
fn unknown_field() {
    let e = fail::<Server>("host \"localhost\"\nprot 8080\n");
    assert_eq!(e.kind(), ErrorKind::UnknownField);
    assert_eq!(e.line_col(), Some((2, 1)));
    insta::assert_snapshot!(render(&e));
}

#[test]
fn missing_field_in_nested_node() {
    let e = fail::<Config>("name \"acme\"\nserver {\n  host \"h\"\n}\n");
    assert_eq!(e.kind(), ErrorKind::MissingField);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn missing_field_at_top_level() {
    let e = fail::<Config>("name \"acme\"\n");
    insta::assert_snapshot!(render(&e));
}

#[test]
fn wrong_type() {
    let e = fail::<Server>("host \"h\"\nport \"eighty\"\n");
    assert_eq!(e.kind(), ErrorKind::InvalidType);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn out_of_range() {
    let e = fail::<Server>("host \"h\"\nport 70000\n");
    assert_eq!(e.kind(), ErrorKind::InvalidValue);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn duplicate_scalar_node() {
    let e = fail::<Server>("host \"a\"\nport 1\nhost \"b\"\n");
    assert_eq!(e.kind(), ErrorKind::DuplicateNode);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn too_many_arguments() {
    let e = fail::<Server>("host \"a\" \"b\"\nport 1\n");
    insta::assert_snapshot!(render(&e));
}

#[test]
fn missing_value() {
    let e = fail::<Server>("host\nport 1\n");
    insta::assert_snapshot!(render(&e));
}

#[test]
fn unknown_variant() {
    let e =
        fail::<Config>("name \"acme\"\nserver host=\"h\" port=1\nmember \"ada\" role=\"admn\"\n");
    assert_eq!(e.kind(), ErrorKind::UnknownVariant);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn unknown_property() {
    let e = fail::<Config>(
        "name \"acme\"\nserver host=\"h\" port=1\nmember \"ada\" role=\"admin\" rol=\"x\"\n",
    );
    insta::assert_snapshot!(render(&e));
}

#[test]
fn unexpected_argument() {
    let e = fail::<Config>(
        "name \"acme\"\nserver host=\"h\" port=1\nmember \"ada\" \"lovelace\" role=\"admin\"\n",
    );
    insta::assert_snapshot!(render(&e));
}

#[test]
fn missing_argument() {
    let e = fail::<Config>("name \"acme\"\nserver host=\"h\" port=1\nmember role=\"admin\"\n");
    insta::assert_snapshot!(render(&e));
}

#[test]
fn property_and_child_clash() {
    let e = fail::<Server>("server_ignored 1\n");
    assert_eq!(e.kind(), ErrorKind::UnknownField);
    #[derive(Debug, Deserialize)]
    struct Doc {
        server: Server,
    }
    let e = fail::<Doc>("server host=\"a\" port=1 {\n  host \"b\"\n}\n");
    assert_eq!(e.kind(), ErrorKind::DuplicateField);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn map_entry_without_key() {
    #[derive(Debug, Deserialize)]
    struct Doc {
        site: BTreeMap<String, Server>,
    }
    let e = fail::<Doc>("site \"a\" { host \"h\"; port 1 }\nsite { host \"h\"; port 2 }\n");
    insta::assert_snapshot!(render(&e));
}

#[test]
fn parse_error() {
    let e = fail::<Server>("host \"unterminated\nport 1\n");
    assert_eq!(e.kind(), ErrorKind::Parse);
    insta::assert_snapshot!(render(&e));
}

#[test]
fn display_includes_location() {
    let e = fail::<Server>("host \"h\"\nport \"x\"\n");
    assert_eq!(e.to_string(), "invalid type: string \"x\", expected u16");
    assert_eq!(e.location().as_deref(), Some("config.kdl:2:6"));
    assert_eq!(e.code(), "kdl_serde::invalid_type");
}

#[test]
fn error_in_directory_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("00-server.kdl"),
        "server host=\"h\" port=1\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("team")).unwrap();
    std::fs::write(
        dir.path().join("team/ada.kdl"),
        "member \"ada\" role=\"admin\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("team/bob.kdl"),
        "// bob\nmember \"bob\" role=\"chief\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("01-name.kdl"), "name \"acme\"\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "not kdl {").unwrap();

    let e = kdl_serde::from_dir::<Config>(dir.path(), true).unwrap_err();
    let file = e.file().unwrap().to_owned();
    assert!(file.ends_with("team/bob.kdl"), "{file}");
    assert_eq!(e.line_col(), Some((2, 14)));
    let rendered = render_width(&e, 1000).replace(dir.path().to_str().unwrap(), "<dir>");
    insta::assert_snapshot!(rendered);

    // Not recursive: the members are not read at all.
    let c: Config = kdl_serde::from_dir(dir.path(), false).unwrap();
    assert!(c.member.is_empty());
}

#[test]
fn duplicate_across_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.kdl"), "host \"a\"\nport 1\n").unwrap();
    std::fs::write(dir.path().join("b.kdl"), "host \"b\"\n").unwrap();
    let e = kdl_serde::from_dir::<Server>(dir.path(), false).unwrap_err();
    let rendered = render_width(&e, 1000).replace(dir.path().to_str().unwrap(), "<dir>");
    insta::assert_snapshot!(rendered);
}
