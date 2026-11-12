//! Benchmarks over realistic inputs: run with
//! `nix develop -c cargo bench -p kdl-serde`.

#![allow(clippy::unwrap_used, missing_docs)]

use std::collections::BTreeMap;
use std::sync::LazyLock;

use divan::{Bencher, black_box};
use serde::{Deserialize, Serialize};

fn main() {
    divan::main();
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Config {
    org: Vec<Org>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Org {
    #[serde(rename = "$0")]
    slug: String,
    display_name: String,
    description: String,
    founded: u16,
    #[serde(rename = "@plan")]
    plan: String,
    member: Vec<Member>,
    site: BTreeMap<String, Site>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Member {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "@role")]
    role: String,
    email: Vec<String>,
    tags: Vec<String>,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Site {
    region: String,
    replicas: u32,
    weight: f64,
    domains: Vec<String>,
    env: BTreeMap<String, String>,
}

fn config(orgs: usize) -> Config {
    Config {
        org: (0..orgs)
            .map(|o| Org {
                slug: format!("org-{o}"),
                display_name: format!("Organization number {o}"),
                description: "A fairly long description of the organization.\nIt spans lines.\n"
                    .into(),
                founded: 1990 + (o % 30) as u16,
                plan: "team".into(),
                member: (0..10)
                    .map(|m| Member {
                        name: format!("user{o}x{m}"),
                        role: ["owner", "admin", "viewer"][m % 3].into(),
                        email: vec![format!("user{m}@org{o}.example")],
                        tags: vec!["a".into(), "bb".into(), "ccc".into()],
                        active: m % 2 == 0,
                    })
                    .collect(),
                site: (0..3)
                    .map(|s| {
                        (
                            format!("site{s}"),
                            Site {
                                region: format!("region-{s}"),
                                replicas: s as u32 + 1,
                                weight: 0.5 * s as f64,
                                domains: vec![format!("s{s}.org{o}.example")],
                                env: BTreeMap::from([
                                    ("LOG_LEVEL".into(), "info".into()),
                                    ("FEATURE".into(), "on".into()),
                                ]),
                            },
                        )
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// About 1 MB of settings-like KDL.
static BIG: LazyLock<(Config, String)> = LazyLock::new(|| {
    let c = config(560);
    let text = kdl_serde::to_string_pretty(&c).unwrap();
    eprintln!("big document: {} bytes", text.len());
    (c, text)
});

/// 20 000 small top-level nodes.
static SMALL: LazyLock<String> =
    LazyLock::new(|| (0..20_000).map(|i| format!("key{i} {i}\n")).collect());

#[derive(Debug, Serialize, Deserialize)]
struct Nest {
    #[serde(rename = "$0")]
    depth: u32,
    a: Option<Box<Nest>>,
}

/// 100 levels of nesting.
static DEEP: LazyLock<String> = LazyLock::new(|| {
    let mut n = Nest {
        depth: 100,
        a: None,
    };
    for d in (0..100).rev() {
        n = Nest {
            depth: d,
            a: Some(Box::new(n)),
        };
    }
    kdl_serde::to_string_pretty(&BTreeMap::from([("a", n)])).unwrap()
});

/// 500 files of 2 orgs each.
static DIR: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..500 {
        let mut c = config(2);
        for o in &mut c.org {
            o.slug = format!("{}-{i}", o.slug);
        }
        std::fs::write(
            dir.path().join(format!("{i:03}.kdl")),
            kdl_serde::to_string_pretty(&c).unwrap(),
        )
        .unwrap();
    }
    dir
});

#[divan::bench]
fn parse_only_big(b: Bencher) {
    let text = &BIG.1;
    b.bench(|| kdl_tree::Document::parse(black_box(text)).unwrap());
}

/// The `kdl` crate's parse of the same text, for comparison.
#[divan::bench]
fn parse_only_big_kdl(b: Bencher) {
    let text = &BIG.1;
    b.bench(|| kdl::KdlDocument::parse_v2(black_box(text)).unwrap());
}

#[divan::bench]
fn load_str_big(b: Bencher) {
    let text = &BIG.1;
    b.bench(|| kdl_load::load_str("", black_box(text.as_str())).unwrap());
}

#[divan::bench]
fn from_str_big(b: Bencher) {
    let text = &BIG.1;
    b.bench(|| kdl_serde::from_str::<Config>(black_box(text)).unwrap());
}

#[divan::bench]
fn deserialize_only_big(b: Bencher) {
    let loaded = kdl_load::load_str("", BIG.1.as_str()).unwrap();
    b.bench(|| kdl_serde::from_loaded::<Config>(black_box(&loaded)).unwrap());
}

#[divan::bench]
fn to_doc_big(b: Bencher) {
    let c = &BIG.0;
    b.bench(|| kdl_serde::to_doc(black_box(c)).unwrap());
}

#[divan::bench]
fn to_string_pretty_big(b: Bencher) {
    let c = &BIG.0;
    b.bench(|| kdl_serde::to_string_pretty(black_box(c)).unwrap());
}

#[divan::bench]
fn pretty_format_big(b: Bencher) {
    let doc = kdl::KdlDocument::parse_v2(&BIG.1).unwrap();
    let opts = kdl_pretty::Options::default();
    b.bench(|| kdl_pretty::format(black_box(&doc), &opts));
}

#[divan::bench]
fn from_str_small_nodes(b: Bencher) {
    let text = &*SMALL;
    b.bench(|| kdl_serde::from_str::<BTreeMap<String, i64>>(black_box(text)).unwrap());
}

#[divan::bench]
fn deserialize_only_small_nodes(b: Bencher) {
    let loaded = kdl_load::load_str("", SMALL.as_str()).unwrap();
    b.bench(|| kdl_serde::from_loaded::<BTreeMap<String, i64>>(black_box(&loaded)).unwrap());
}

#[divan::bench]
fn from_str_deep(b: Bencher) {
    let text = &*DEEP;
    b.bench(|| kdl_serde::from_str::<BTreeMap<String, Nest>>(black_box(text)).unwrap());
}

#[divan::bench(sample_count = 20)]
fn load_dir_500(b: Bencher) {
    let dir = DIR.path();
    b.bench(|| kdl_load::load_dir(black_box(dir), false).unwrap());
}

#[divan::bench(sample_count = 20)]
fn from_dir_500(b: Bencher) {
    let dir = DIR.path();
    b.bench(|| kdl_serde::from_dir::<Config>(black_box(dir), false).unwrap());
}
