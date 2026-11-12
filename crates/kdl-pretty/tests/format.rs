//! Fixtures, idempotence, comment preservation and semantics.

use kdl::KdlDocument;
use kdl_pretty::{Options, format, format_compact, format_str};
use proptest::prelude::*;

fn fmt(src: &str) -> String {
    format_str(src, &Options::default()).expect("parses")
}

fn semantics(src: &str) -> KdlDocument {
    let mut doc = KdlDocument::parse_v2(src).expect("parses");
    doc.clear_format_recursive();
    doc
}

/// Formatting is idempotent and keeps the document's meaning.
fn check(src: &str) -> String {
    let out = fmt(src);
    assert_eq!(fmt(&out), out, "not idempotent:\n{out}");
    assert_eq!(semantics(src), semantics(&out), "meaning changed:\n{out}");
    out
}

#[test]
fn fixtures() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).expect("fixtures dir") {
        let path = entry.expect("entry").path();
        let name = path.to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".kdl") else {
            continue;
        };
        if stem.ends_with(".formatted") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        let out = check(&src);
        let expected_path = format!("{stem}.formatted.kdl");
        if std::env::var_os("KDL_PRETTY_BLESS").is_some() {
            std::fs::write(&expected_path, &out).expect("write");
        }
        let expected = std::fs::read_to_string(&expected_path).expect("expected output");
        assert_eq!(out, expected, "{name}");
        // The expected output is a fixed point.
        assert_eq!(fmt(&expected), expected, "{expected_path}");
        seen += 1;
    }
    assert!(seen >= 4);
}

#[test]
fn comments_before_after_and_inside() {
    let src =
        "// top\n\n\n/* own */\na 1 // after a\nb {\n// first\nc\n// last\n} // after b\n// end\n";
    assert_eq!(
        check(src),
        "// top\n\n/* own */\na 1 // after a\nb {\n  // first\n  c\n  // last\n} // after b\n// end\n"
    );
}

#[test]
fn block_comment_glued_to_node() {
    assert_eq!(check("/* x */ a\n/* y */\nb"), "/* x */ a\n/* y */\nb\n");
}

#[test]
fn comment_after_semicolon_stays_with_its_node() {
    assert_eq!(check("a; // a\nb; /* b */ c"), "a // a\nb /* b */\nc\n");
}

#[test]
fn comment_after_open_brace() {
    assert_eq!(check("a { // hi\n  b\n}"), "a { // hi\n  b\n}\n");
}

#[test]
fn slashdashes() {
    assert_eq!(
        check("/-a 1\nb /-1 2 /-k=v /-{ c } { d }\ne /- 3 /-(t)\"x\"\nf {\n  /-g { h }\n}"),
        "/-a 1\nb /-1 2 /-k=\"v\" /-{\n  c\n} {\n  d\n}\ne /-3 /-(t)\"x\"\nf {\n  /-g {\n    h\n  }\n}\n"
    );
}

#[test]
fn blank_lines_collapse() {
    assert_eq!(check("\n\na\n\n\n\nb\nc\n\n\n"), "a\n\nb\nc\n");
    assert_eq!(check("a {\n\n  b\n\n\n  c\n\n}"), "a {\n  b\n\n  c\n}\n");
}

#[test]
fn line_continuation_comment() {
    assert_eq!(check("a 1 \\ // why\n  2"), "a 1 \\ // why\n  2\n");
    assert_eq!(check("a 1 \\\n  2"), "a 1 2\n");
}

#[test]
fn empty_children_kept() {
    assert_eq!(check("a {}\nb { }"), "a {}\nb {}\n");
    assert_eq!(
        check("a { b } /* x */ /* y */;c"),
        "a {\n  b\n} /* x */ /* y */\nc\n"
    );
}

#[test]
fn multiline_strings() {
    assert_eq!(
        check("a {\n  b \"x\\n  y\\n\"\n}"),
        "a {\n  b \"\"\"\n    x\n      y\n\n    \"\"\"\n}\n"
    );
    assert_eq!(check("a \"t  \\n\""), "a \"\"\"\n  t\\s\\s\n\n  \"\"\"\n");
}

#[test]
fn raw_strings_stay_raw() {
    assert_eq!(check(r##"a #"x\y"#"##), "a #\"x\\y\"#\n");
    assert_eq!(check(r###"a ##"say "#hi"##"###), "a ##\"say \"#hi\"##\n");
    assert_eq!(
        check("a {\n b #\"\"\"\n     x\\n\n       y\n     \"\"\"#\n}"),
        "a {\n  b #\"\"\"\n    x\\n\n      y\n    \"\"\"#\n}\n"
    );
    // A whitespace-only line can't be raw; fall back to escapes.
    assert_eq!(
        check("a #\"\"\"\n  x\n  \\s\n  \"\"\"#"),
        "a #\"\"\"\n  x\n  \\s\n  \"\"\"#\n"
    );
}

#[test]
fn programmatic_documents() {
    let mut doc = KdlDocument::new();
    let mut node = kdl::KdlNode::new("a b");
    node.push(kdl::KdlEntry::new_prop("k", 1.5));
    node.push("multi\nline");
    node.set_children(KdlDocument::new());
    doc.nodes_mut().push(node);
    let opts = Options::default().indent(4);
    assert_eq!(
        format(&doc, &opts),
        "\"a b\" k=1.5 \"\"\"\n    multi\n    line\n    \"\"\" {}\n"
    );
    assert_eq!(format_compact(&doc), "\"a b\" k=1.5 \"multi\\nline\"\n");
}

#[test]
fn compact_drops_comments() {
    let doc = KdlDocument::parse_v2("// x\na 1 { b /* y */ 2; c }").expect("parses");
    assert_eq!(format_compact(&doc), "a 1 { b 2; c; }\n");
}

#[test]
fn parse_errors() {
    assert!(format_str("a {", &Options::default()).is_err());
}

fn ident() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec!["a", "node", "x-y", "été", "$k", "@p"]).prop_map(String::from),
        prop::sample::select(vec!["\"a b\"", "\"1\"", "\"true\"", "#\"r\"#"])
            .prop_map(String::from),
    ]
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn value() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<i32>().prop_map(|i| i.to_string()),
        (-1e6f64..1e6).prop_map(|f| format!("{f:?}")),
        prop::sample::select(vec!["#true", "#false", "#null", "#inf", "0xff", "1e3"])
            .prop_map(String::from),
        "[a-z \"\\\\\n\t#]{0,12}".prop_map(|s| quote(&s)),
        "[a-z \\\\\"]{0,8}".prop_map(|s| format!("##\"{s}\"##")),
        prop::collection::vec("[a-z][a-z \"\\\\]{0,6}", 1..4)
            .prop_map(|ls| format!("#\"\"\"\n  {}\n  \"\"\"#", ls.join("\n  "))),
        prop::collection::vec("[a-z ]{0,6}", 1..4)
            .prop_map(|ls| format!("\"\"\"\n{}\n\"\"\"", ls.join("\n"))),
    ]
}

fn entry() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["", "", "/-", "/* c */ "]),
        prop::option::of(ident()),
        prop::option::of(Just("(t)")),
        value(),
    )
        .prop_map(|(pre, key, ty, v)| {
            let key = key.map(|k| format!("{k}=")).unwrap_or_default();
            format!("{pre}{key}{}{v}", ty.unwrap_or_default())
        })
}

fn node_text() -> impl Strategy<Value = String> {
    let leaf = (
        prop::sample::select(vec!["", "", "/-", "// note\n", "/* c */ ", "\n\n"]),
        ident(),
        prop::collection::vec(entry(), 0..4),
        prop::sample::select(vec!["\n", ";", " // trailing\n", ";\n"]),
    )
        .prop_map(|(pre, name, entries, term)| {
            let mut s = format!("{pre}{name}");
            for e in entries {
                s.push(' ');
                s.push_str(&e);
            }
            s.push_str(term);
            s
        });
    leaf.prop_recursive(3, 24, 4, |inner| {
        (
            prop::sample::select(vec!["", "/-", "// above\n"]),
            ident(),
            prop::collection::vec(inner, 0..4),
            prop::sample::select(vec!["", "/-{ gone }", "// inside\n"]),
            prop::sample::select(vec!["\n", " // after\n", ";"]),
        )
            .prop_map(|(pre, name, kids, extra, term)| {
                format!("{pre}{name} {{\n{}{extra}\n}}{term}", kids.concat())
            })
    })
}

fn count(s: &str, pat: &str) -> usize {
    s.matches(pat).count()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn idempotent_and_meaning_preserving(
        nodes in prop::collection::vec(node_text(), 0..6),
        tail in prop::sample::select(vec!["", "// end\n", "/* end */"]),
        indent in 0usize..5,
    ) {
        let src = format!("{}{tail}", nodes.concat());
        let Ok(doc) = KdlDocument::parse_v2(&src) else {
            // The generator can glue tokens into something invalid.
            return Ok(());
        };
        let opts = Options::default().indent(indent);
        let out = format_str(&src, &opts).expect("formats");
        prop_assert_eq!(format_str(&out, &opts).expect("reparses"), out.clone());
        let mut before = doc;
        before.clear_format_recursive();
        prop_assert_eq!(semantics(&out), before);
        // Generated strings hold no `/`, so these count comments.
        for pat in ["//", "/*", "/-"] {
            prop_assert_eq!(count(&src, pat), count(&out, pat), "{} in\n{}\n---\n{}", pat, src, out);
        }
        // Programmatic formatting (no source) agrees on meaning too.
        let plain = format(&semantics(&src), &opts);
        prop_assert_eq!(semantics(&plain), semantics(&src));
    }
}
