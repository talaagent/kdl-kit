//! Realistic documents: read, check, write (snapshot), read back.

#![allow(clippy::unwrap_used)]

extern crate alloc;

use alloc::collections::BTreeMap;

use kdl_serde::Annotated;
use serde::{Deserialize, Serialize};

fn read<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    match kdl_serde::from_file(&path) {
        Ok(v) => v,
        Err(e) => panic!("{e:?}"),
    }
}

/// Writes `v`, reads it back, and returns the text.
fn rewrite<T>(v: &T) -> String
where
    T: Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
{
    let text = kdl_serde::to_string_pretty(v).unwrap();
    let back: T = kdl_serde::from_str(&text).unwrap();
    assert_eq!(&back, v);
    text
}

// --- org.kdl ---------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrgFile {
    org: Vec<Org>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Org {
    #[serde(rename = "$0")]
    slug: String,
    display_name: String,
    founded: u16,
    billing: Billing,
    member: Vec<Member>,
    site: BTreeMap<String, Site>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Billing {
    #[serde(rename = "@plan")]
    plan: String,
    #[serde(rename = "@seats")]
    seats: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "@role", default = "viewer")]
    role: Role,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    email: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

fn viewer() -> Role {
    Role::Viewer
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    Owner,
    Admin,
    Viewer,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Site {
    region: String,
    replicas: u32,
    domains: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
}

#[test]
fn org() {
    let file: OrgFile = read("org.kdl");
    let org = &file.org[0];
    assert_eq!(org.slug, "acme");
    assert_eq!(org.billing.seats, 25);
    assert_eq!(org.member.len(), 3);
    assert_eq!(org.member[0].email.len(), 2);
    assert_eq!(org.member[2].role, Role::Viewer);
    assert_eq!(org.site["eu"].env["LOG_LEVEL"], "info");
    assert_eq!(org.site["us"].domains, ["acme.com"]);
    insta::assert_snapshot!(rewrite(&file));
}

// --- settings.kdl ----------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SettingsFile {
    settings: Settings,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Settings {
    theme: Theme,
    font_size: u8,
    autosave: bool,
    autosave_interval: f32,
    max_upload: Annotated<u64>,
    proxy: Option<String>,
    editor: Editor,
    keybindings: BTreeMap<String, String>,
    plugins: Vec<String>,
    motd: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
    Light,
    Dark,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Editor {
    tab_width: u8,
    soft_wrap: bool,
}

#[test]
fn settings() {
    let file: SettingsFile = read("settings.kdl");
    let s = &file.settings;
    assert_eq!(s.theme, Theme::Dark);
    assert_eq!(s.max_upload, Annotated::new("bytes", 10_485_760));
    assert_eq!(s.proxy, None);
    assert_eq!(s.editor.tab_width, 4);
    assert_eq!(s.keybindings["quit"], "ctrl+q");
    assert_eq!(s.plugins, ["git", "lsp"]);
    assert_eq!(s.motd, "Welcome back!\n  Remember: \"measure twice\".");
    insta::assert_snapshot!(rewrite(&file));
}

// --- schema.kdl ------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Item {
    Struct(StructDef),
    Enum(EnumDef),
    Alias(AliasDef),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StructDef {
    #[serde(rename = "$0")]
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    doc: Option<String>,
    field: Vec<FieldDef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDef {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "$1")]
    ty: Annotated<String>,
    #[serde(rename = "@required", default, skip_serializing_if = "is_false")]
    required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    doc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnumDef {
    #[serde(rename = "$0")]
    name: String,
    variant: Vec<VariantDef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantDef {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "@value", default, skip_serializing_if = "Option::is_none")]
    value: Option<i64>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasDef {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "$1")]
    target: String,
}

#[test]
fn schema() {
    let items: Vec<Item> = read("schema.kdl");
    let Item::Struct(foo) = &items[0] else {
        panic!("{items:?}")
    };
    assert_eq!(foo.name, "Foo");
    assert_eq!(foo.field.len(), 4);
    assert!(foo.field[0].required);
    assert_eq!(foo.field[1].default.as_deref(), Some("unnamed"));
    assert_eq!(foo.field[2].ty, Annotated::new("list", "string".into()));
    let Item::Enum(color) = &items[1] else {
        panic!("{items:?}")
    };
    assert_eq!(color.variant[1].value, Some(2));
    assert!(matches!(&items[2], Item::Alias(a) if a.target == "u64"));
    insta::assert_snapshot!(rewrite(&items));
}

#[test]
fn schema_as_raw_document_keeps_everything() {
    let raw: kdl_serde::RawDocument = read("schema.kdl");
    let text = kdl_pretty::format(&raw.0, &kdl_pretty::Options::default());
    let again: Vec<Item> = kdl_serde::from_str(&text).unwrap();
    assert_eq!(again, read::<Vec<Item>>("schema.kdl"));
}
