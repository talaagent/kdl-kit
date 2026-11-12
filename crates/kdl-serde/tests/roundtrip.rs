//! Property tests: anything serializable reads back equal, in every style.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use kdl_serde::{Annotated, MapStyle, Options, SeqStyle};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Unit,
    Newtype(i32),
    Text(String),
    Tuple(u8, String),
    Struct { a: i64, b: Option<String> },
    List(Vec<i16>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Leaf {
    #[serde(rename = "$0")]
    id: String,
    #[serde(rename = "@weight")]
    weight: i32,
    #[serde(rename = "@note", default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(default)]
    flag: bool,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Doc {
    text: String,
    small: u8,
    signed: i64,
    big: u64,
    float: f64,
    single: f32,
    ch: char,
    yes: bool,
    maybe: Option<i32>,
    unit: (),
    pair: (i32, String),
    strings: Vec<String>,
    ints: Vec<i64>,
    grid: Vec<Vec<u8>>,
    leaf: Vec<Leaf>,
    by_name: BTreeMap<String, Leaf>,
    counts: BTreeMap<String, i64>,
    by_num: BTreeMap<u16, String>,
    kind: Kind,
    kinds: Vec<Kind>,
    opt_list: Option<Vec<String>>,
    sparse: Vec<Option<i32>>,
    note: Annotated<String>,
    nested: Option<Box<Doc>>,
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z][a-z0-9_-]{0,8}",
        any::<String>(),
        // Multiline, with indentation, quotes and backslashes.
        proptest::collection::vec("[ \t]{0,3}[a-z\"\\\\ ]{0,6}[ \t]{0,2}", 0..5)
            .prop_map(|lines| lines.join("\n")),
    ]
}

fn float() -> impl Strategy<Value = f64> {
    prop_oneof![
        any::<f64>().prop_filter("NaN is not equal to itself", |f| !f.is_nan()),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(-0.0),
    ]
}

fn kind() -> impl Strategy<Value = Kind> {
    prop_oneof![
        Just(Kind::Unit),
        any::<i32>().prop_map(Kind::Newtype),
        text().prop_map(Kind::Text),
        (any::<u8>(), text()).prop_map(|(a, b)| Kind::Tuple(a, b)),
        (any::<i64>(), proptest::option::of(text())).prop_map(|(a, b)| Kind::Struct { a, b }),
        proptest::collection::vec(any::<i16>(), 0..4).prop_map(Kind::List),
    ]
}

fn leaf() -> impl Strategy<Value = Leaf> {
    (
        text(),
        any::<i32>(),
        proptest::option::of(text()),
        any::<bool>(),
        proptest::collection::vec(text(), 0..3),
    )
        .prop_map(|(id, weight, note, flag, tags)| Leaf {
            id,
            weight,
            note,
            flag,
            tags,
        })
}

fn ident() -> impl Strategy<Value = String> {
    prop_oneof!["[a-z][a-z0-9-]{0,6}", any::<String>()]
}

prop_compose! {
    fn doc_base()(
        text in text(),
        small in any::<u8>(),
        signed in any::<i64>(),
        big in any::<u64>(),
        float in float(),
        single in any::<f32>().prop_filter("not NaN", |f| !f.is_nan()),
        ch in any::<char>(),
        yes in any::<bool>(),
        maybe in proptest::option::of(any::<i32>()),
        pair in (any::<i32>(), text()),
        strings in proptest::collection::vec(text(), 0..4),
        ints in proptest::collection::vec(any::<i64>(), 0..4),
        grid in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..3), 0..3),
    ) -> Doc {
        Doc {
            text, small, signed, big, float, single, ch, yes, maybe, unit: (), pair,
            strings, ints, grid,
            leaf: vec![], by_name: BTreeMap::new(), counts: BTreeMap::new(),
            by_num: BTreeMap::new(), kind: Kind::Unit, kinds: vec![], opt_list: None,
            sparse: vec![], note: Annotated::plain(String::new()), nested: None,
        }
    }
}

prop_compose! {
    fn doc()(
        base in doc_base(),
        leaf in proptest::collection::vec(leaf(), 0..3),
        by_name in proptest::collection::btree_map(ident(), leaf(), 0..3),
        counts in proptest::collection::btree_map(ident(), any::<i64>(), 0..3),
        by_num in proptest::collection::btree_map(any::<u16>(), text(), 0..3),
        kind in kind(),
        kinds in proptest::collection::vec(kind(), 0..4),
        opt_list in proptest::option::of(proptest::collection::vec(text(), 0..3)),
        sparse in proptest::collection::vec(proptest::option::of(any::<i32>()), 0..4),
        note in (proptest::option::of(ident()), text()),
        nested in proptest::option::of(doc_base()),
    ) -> Doc {
        Doc {
            leaf, by_name, counts, by_num, kind, kinds, opt_list, sparse,
            note: Annotated { ty: note.0, value: note.1 },
            nested: nested.map(Box::new),
            ..base
        }
    }
}

fn styles() -> Vec<Options> {
    let mut all = Vec::new();
    for seq in [SeqStyle::Args, SeqStyle::Repeated] {
        for map in [MapStyle::Keyed, MapStyle::Block] {
            for multiline in [true, false] {
                all.push(
                    Options::default()
                        .seq_style(seq)
                        .map_style(map)
                        .multiline_strings(multiline),
                );
            }
        }
    }
    all
}

fn check(doc: &Doc) -> Result<(), TestCaseError> {
    for opts in styles() {
        let text = kdl_serde::to_string_with(doc, &opts).unwrap();
        let back: Doc = kdl_serde::from_str(&text)
            .map_err(|e| TestCaseError::fail(format!("{:?}\n{text}", e)))?;
        prop_assert_eq!(&back, doc, "{:?}\n{}", opts, text);
        // Formatting is deterministic and stable.
        let again = kdl_serde::to_string_with(&back, &opts).unwrap();
        prop_assert_eq!(&again, &text);
    }
    let compact = kdl_serde::to_string(doc).unwrap();
    let back: Doc = kdl_serde::from_str(&compact)
        .map_err(|e| TestCaseError::fail(format!("{:?}\n{compact}", e)))?;
    prop_assert_eq!(&back, doc);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn documents_round_trip(doc in doc()) {
        check(&doc)?;
    }

    #[test]
    fn strings_round_trip_through_the_printer(s in any::<String>(), lines in proptest::collection::vec(any::<String>(), 0..4)) {
        for s in [s, lines.join("\n")] {
            let mut doc = kdl::KdlDocument::new();
            let mut node = kdl::KdlNode::new(s.as_str());
            node.push(kdl::KdlEntry::new(s.as_str()));
            node.push(kdl::KdlEntry::new_prop(s.as_str(), s.as_str()));
            doc.nodes_mut().push(node);
            let text = kdl_pretty::format(&doc, &kdl_pretty::Options::default());
            let parsed: kdl::KdlDocument = text.parse().map_err(|e: kdl::KdlError| {
                TestCaseError::fail(format!("{:?}\n{text}", e))
            })?;
            let n = &parsed.nodes()[0];
            prop_assert_eq!(n.name().value(), s.as_str());
            prop_assert_eq!(n.entries()[0].value().as_string(), Some(s.as_str()));
            prop_assert_eq!(n.entries()[1].value().as_string(), Some(s.as_str()));
        }
    }
}
