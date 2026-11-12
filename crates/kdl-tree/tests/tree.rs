//! Spans, errors and the depth cap.

#![allow(clippy::unwrap_used)]

use kdl_tree::{Document, ErrorKind, ParseOptions, Span};

fn text(src: &str, s: Span) -> &str {
    &src[s.offset..s.end()]
}

#[test]
fn spans_cover_every_part() {
    let src = "// c\n(t) \"no de\" 1  k = (u)\"v\" /-x (a)#true {\n  c 1; d\n}  ;\nlast\n";
    let doc = Document::parse(src).unwrap();
    let n = &doc.nodes()[0];
    assert_eq!(text(src, n.ty().unwrap().span()), "t");
    assert_eq!(text(src, n.name().span()), "\"no de\"");
    assert_eq!(n.name().value(), "no de");
    let e: Vec<_> = n.entries().iter().map(|e| text(src, e.span())).collect();
    assert_eq!(e, ["1", "k = (u)\"v\"", "(a)#true"]);
    let k = &n.entries()[1];
    assert_eq!(text(src, k.name().unwrap().span()), "k");
    assert_eq!(text(src, k.ty().unwrap().span()), "u");
    assert_eq!(text(src, k.value_span()), "\"v\"");
    assert_eq!(text(src, n.entries()[2].value_span()), "#true");
    let children = n.children().unwrap();
    assert_eq!(text(src, children.span()), "{\n  c 1; d\n}");
    assert_eq!(text(src, n.span()), &src[5..src.find("}").unwrap() + 1]);
    assert_eq!(text(src, children.nodes()[0].span()), "c 1");
    assert_eq!(text(src, doc.nodes()[1].span()), "last");
}

#[test]
fn base_shifts_every_offset() {
    let src = "a 1 { b k=2 }";
    let doc = Document::parse_with(src, &ParseOptions::default().base(100)).unwrap();
    let b = &doc.nodes()[0].children().unwrap().nodes()[0];
    assert_eq!(b.name().span(), Span::new(106, 1));
    assert_eq!(b.entries()[0].value_span(), Span::new(110, 1));
    let err = Document::parse_with("a {", &ParseOptions::default().base(100)).unwrap_err();
    assert_eq!(err.span(), Span::new(102, 1));
}

#[test]
fn borrows_plain_strings() {
    let doc = Document::parse(r##"node "plain" bare #"raw"# "esc\n""##).unwrap();
    let borrowed: Vec<bool> = doc.nodes()[0]
        .entries()
        .iter()
        .map(|e| {
            matches!(
                e.value(),
                kdl_tree::Value::String(std::borrow::Cow::Borrowed(_))
            )
        })
        .collect();
    assert_eq!(borrowed, [true, true, true, false]);
}

#[test]
fn errors_have_codes_and_offsets() {
    let cases = [
        ("a \"x", ErrorKind::Unterminated, 2, "unterminated string"),
        (
            "a\nb true",
            ErrorKind::InvalidIdentifier,
            4,
            "bare `true` is not a value",
        ),
        (
            "a 0x",
            ErrorKind::InvalidNumber,
            2,
            "expected a digit after the radix prefix",
        ),
        (
            "a { b } c",
            ErrorKind::ChildrenOrder,
            8,
            "entries must come before the children block",
        ),
        ("a #nope", ErrorKind::InvalidKeyword, 2, "unknown keyword"),
        (
            "a \"\\q\"",
            ErrorKind::InvalidEscape,
            3,
            "invalid escape `\\q`",
        ),
        ("}", ErrorKind::Unexpected, 0, "unexpected `}`"),
    ];
    for (src, kind, offset, message) in cases {
        let e = Document::parse(src).unwrap_err();
        assert_eq!(
            (e.kind(), e.span().offset, e.message()),
            (kind, offset, message),
            "{src}"
        );
        assert!(e.code().starts_with("kdl_tree::"));
    }
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    for depth in [257, 10_000, 100_000] {
        let src = "a {".repeat(depth) + &"}".repeat(depth);
        let e = Document::parse(&src).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::TooDeep);
        assert_eq!(e.span().offset, 256 * 3 + 2);
    }
    let src = "a {".repeat(256) + &"}".repeat(256);
    assert!(Document::parse(&src).is_ok());
    let opts = ParseOptions::default().max_depth(2);
    assert!(Document::parse_with("a { b { c } }", &opts).is_ok());
    let e = Document::parse_with("a { b { c { } } }", &opts).unwrap_err();
    assert_eq!(e.code(), "kdl_tree::too_deep");
}

#[test]
fn into_owned_keeps_everything() {
    let src = String::from("(t)a \"x\" k=1 { b }");
    let doc = Document::parse(&src).unwrap();
    let owned: Document<'static> = doc.clone().into_owned();
    drop(src);
    assert_eq!(owned, doc_eq_helper());
    fn doc_eq_helper() -> Document<'static> {
        Document::parse("(t)a \"x\" k=1 { b }")
            .unwrap()
            .into_owned()
    }
}

#[test]
fn kdl_round_trip_keeps_spans() {
    let src = "(t)a \"x\" k=(u)1 { b }";
    let doc = Document::parse(src).unwrap();
    let k = doc.to_kdl();
    assert_eq!(k.nodes()[0].entries()[1].span().offset(), 9);
    let back = Document::from_kdl(&k);
    assert_eq!(
        back.nodes()[0].entries()[1].span(),
        doc.nodes()[0].entries()[1].span()
    );
    assert_eq!(back.nodes()[0].name().span(), doc.nodes()[0].name().span());
}
