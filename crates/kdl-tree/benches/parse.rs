//! `kdl_tree::Document::parse` against `kdl::KdlDocument::parse_v2` on the
//! same inputs, in bytes per second: run with
//! `nix develop -c cargo bench -p kdl-tree --bench parse`.
//!
//! Both sides do the same work: parse UTF-8 text into a document, or fail.

#![allow(clippy::unwrap_used, missing_docs)]

use core::fmt::Write as _;
use std::sync::LazyLock;

use divan::counter::BytesCount;
use divan::{Bencher, black_box};

fn main() {
    // Both parsers must accept every input, or the comparison is moot.
    for name in INPUTS {
        let text = input(name);
        kdl_tree::Document::parse(text).unwrap();
        kdl::KdlDocument::parse_v2(text).unwrap();
        eprintln!("{name}: {} bytes", text.len());
    }
    divan::main();
}

const INPUTS: [&str; 5] = ["settings", "siblings", "deep", "strings", "numbers"];

fn input(name: &str) -> &'static str {
    match name {
        "settings" => &SETTINGS,
        "siblings" => &SIBLINGS,
        "deep" => &DEEP,
        "strings" => &STRINGS,
        "numbers" => &NUMBERS,
        _ => unreachable!("unknown input {name}"),
    }
}

/// About 1 MB of settings-style KDL, the shape kdl-serde's benches write:
/// named blocks with properties, children, a multiline string, lists and
/// maps.
static SETTINGS: LazyLock<String> = LazyLock::new(|| {
    let mut s = String::new();
    let mut o = 0;
    while s.len() < 1_000_000 {
        writeln!(s, "org \"org-{o}\" plan=\"team\" {{").unwrap();
        writeln!(s, "  display-name \"Organization number {o}\"").unwrap();
        s.push_str(
            "  description \"\"\"\n    A fairly long description of the organization.\n    It spans lines.\n    \"\"\"\n",
        );
        writeln!(s, "  founded {}", 1990 + o % 30).unwrap();
        for m in 0..10 {
            let role = ["owner", "admin", "viewer"][m % 3];
            writeln!(s, "  member user{o}x{m} role={role} {{").unwrap();
            writeln!(s, "    email \"user{m}@org{o}.example\"").unwrap();
            s.push_str("    tags a bb ccc\n");
            writeln!(s, "    active #{}", m % 2 == 0).unwrap();
            s.push_str("  }\n");
        }
        for t in 0..3 {
            writeln!(s, "  site site{t} {{").unwrap();
            writeln!(s, "    region \"region-{t}\"").unwrap();
            writeln!(s, "    replicas {}", t + 1).unwrap();
            writeln!(s, "    weight {:?}", 0.5 * f64::from(t)).unwrap();
            writeln!(s, "    domains \"s{t}.org{o}.example\"").unwrap();
            s.push_str("    env FEATURE on\n    env LOG_LEVEL info\n  }\n");
        }
        s.push_str("}\n");
        o += 1;
    }
    s
});

/// 20 000 small top-level nodes.
static SIBLINGS: LazyLock<String> =
    LazyLock::new(|| (0..20_000).map(|i| format!("key{i} {i}\n")).collect());

/// 100 levels of nesting, two nodes per level.
static DEEP: LazyLock<String> = LazyLock::new(|| {
    let mut s = String::new();
    for d in 0..100 {
        let pad = "  ".repeat(d);
        writeln!(s, "{pad}a {d} {{").unwrap();
        writeln!(s, "{pad}  leaf \"level {d}\"").unwrap();
    }
    for d in (0..100).rev() {
        writeln!(s, "{}}}", "  ".repeat(d)).unwrap();
    }
    s
});

/// About 1 MB of strings: escapes, unicode escapes, whitespace escapes,
/// raw strings and multiline (raw and escaped) strings.
static STRINGS: LazyLock<String> = LazyLock::new(|| {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < 1_000_000 {
        writeln!(
            s,
            r#"esc{i} "tab\there \"quoted\" back\\slash \u{{1F600}} nl\n" key="a\sb\rc""#
        )
        .unwrap();
        writeln!(
            s,
            r##"raw{i} #"raw "quoted" \n stays C:\path"# "plain {i}""##
        )
        .unwrap();
        writeln!(s, "cont{i} \"line one \\\n      continued\"").unwrap();
        s.push_str(
            "multi \"\"\"\n    first line\n      indented \\t escaped\n    last\n    \"\"\"\n",
        );
        s.push_str("rawmulti #\"\"\"\n    keep \\n and \"quotes\"\n    as written\n    \"\"\"#\n");
        i += 1;
    }
    s
});

/// About 1 MB of numbers: decimal, signed, underscored, hex, octal, binary,
/// floats with exponents, and the keyword floats.
static NUMBERS: LazyLock<String> = LazyLock::new(|| {
    let mut s = String::new();
    let mut i: u64 = 0;
    while s.len() < 1_000_000 {
        writeln!(
            s,
            "n{i} {i} -{i} +{i} 1_000_{:03} 0x{i:x} 0o{i:o} 0b{:b} {i}.25 -{i}.5e10 6.02E+{} 1.0e-7 #inf #-inf #nan",
            i % 1000,
            i % 256,
            i % 30,
        )
        .unwrap();
        i += 1;
    }
    s
});

#[divan::bench(args = INPUTS)]
fn kdl_tree(b: Bencher, name: &str) {
    let text = input(name);
    b.counter(BytesCount::of_str(text))
        .bench(|| kdl_tree::Document::parse(black_box(text)).unwrap());
}

#[divan::bench(args = INPUTS)]
fn kdl(b: Bencher, name: &str) {
    let text = input(name);
    b.counter(BytesCount::of_str(text))
        .bench(|| kdl::KdlDocument::parse_v2(black_box(text)).unwrap());
}
