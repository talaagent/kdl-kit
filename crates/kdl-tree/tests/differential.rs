//! On generated documents, the tree matches `kdl`'s parse: the same
//! meaning, and the same name, type, entry and key spans.

#![allow(clippy::unwrap_used)]

mod common;

use kdl_tree::{Document, Node, Span};
use proptest::prelude::*;

fn ws() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![" ", "  ", "\t", " /* c */ ", " \\\n  ", " \\ // c\n "])
}

fn string() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec!["a", "node", "x-y", "über", "_z", "a.b", "-x", "+y"])
            .prop_map(str::to_owned),
        "[a-z ]{0,6}".prop_map(|s| format!("\"{s}\"")),
        Just("\"esc\\n\\t\\u{1F600}\\\"q\"".to_owned()),
        Just("#\"raw \\ \"q\"#".to_owned()),
        Just("##\"r\"#aw\"##".to_owned()),
        Just("\"\"\"\n    multi\n      line\n    \"\"\"".to_owned()),
    ]
}

fn value() -> impl Strategy<Value = String> {
    prop_oneof![
        string(),
        any::<i64>().prop_map(|i| i.to_string()),
        any::<u16>().prop_map(|i| format!("0x{i:x}")),
        Just("0b1_01".to_owned()),
        Just("-0o17".to_owned()),
        Just("1_000".to_owned()),
        (-1000i32..1000, 0u8..100).prop_map(|(a, b)| format!("{a}.{b}")),
        Just("2.5e-3".to_owned()),
        prop::sample::select(vec!["#true", "#false", "#null", "#inf", "#-inf", "#nan"])
            .prop_map(str::to_owned),
    ]
}

fn ty() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => Just(String::new()),
        1 => string().prop_map(|t| format!("({t})")),
        1 => Just("( u8 )".to_owned()),
    ]
}

fn entry() -> impl Strategy<Value = String> {
    let arg = (ty(), value()).prop_map(|(t, v)| format!("{t}{v}"));
    let prop = (string(), ty(), value(), any::<bool>()).prop_map(|(k, t, v, spaced)| {
        if spaced {
            format!("{k} = {t}{v}")
        } else {
            format!("{k}={t}{v}")
        }
    });
    let e = prop_oneof![arg, prop];
    (any::<u8>(), e).prop_map(|(sd, e)| if sd % 8 == 0 { format!("/-{e}") } else { e })
}

fn terminator() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["\n", ";", " // c\n", "\r\n", "\n\n"])
}

fn node(depth: u32) -> BoxedStrategy<String> {
    let entries = prop::collection::vec((ws(), entry()), 0..4).prop_map(|es| {
        es.into_iter().fold(String::new(), |mut s, (w, e)| {
            s.push_str(w);
            s.push_str(&e);
            s
        })
    });
    let children: BoxedStrategy<String> = if depth == 0 {
        Just(String::new()).boxed()
    } else {
        prop_oneof![
            2 => Just(String::new()),
            1 => Just(" {}".to_owned()),
            2 => prop::collection::vec(node(depth - 1), 0..4)
                .prop_map(|ns| format!(" {{\n{}}}", ns.concat())),
        ]
        .boxed()
    };
    (ty(), string(), entries, children, terminator(), any::<u8>())
        .prop_map(|(t, n, e, c, term, sd)| {
            // `kdl` rejects a slashdashed node ended by `;` (valid KDL).
            if sd % 10 == 0 {
                format!("/- {t}{n}{e}{c}\n")
            } else {
                format!("{t}{n}{e}{c}{term}")
            }
        })
        .boxed()
}

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(node(3), 0..6).prop_map(|ns| ns.concat())
}

fn spans(nodes: &[Node<'_>], out: &mut Vec<(String, Span)>) {
    for n in nodes {
        let name = n.name().value();
        out.push((format!("{name} name"), n.name().span()));
        if let Some(t) = n.ty() {
            out.push((format!("{name} type"), t.span()));
        }
        for (i, e) in n.entries().iter().enumerate() {
            out.push((format!("{name} entry {i}"), e.span()));
            if let Some(k) = e.name() {
                out.push((format!("{name} key {i}"), k.span()));
            }
            if let Some(t) = e.ty() {
                out.push((format!("{name} type {i}"), t.span()));
            }
        }
        if let Some(c) = n.children() {
            spans(c.nodes(), out);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn matches_kdl(src in document()) {
        let oracle = kdl::KdlDocument::parse_v2(&src);
        let ours = Document::parse(&src);
        let (oracle, ours) = match (oracle, ours) {
            (Ok(a), Ok(b)) => (a, b),
            (a, b) => panic!("parse failed: {src:?}\nkdl: {:?}\nours: {:?}", a.err(), b.err()),
        };
        let want = Document::from_kdl(&oracle);
        prop_assert_eq!(common::model(&ours), common::model(&want), "{:?}", src);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        spans(ours.nodes(), &mut a);
        spans(want.nodes(), &mut b);
        prop_assert_eq!(a, b, "{:?}", src);
    }
}
