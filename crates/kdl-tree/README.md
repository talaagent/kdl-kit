# kdl-tree

A fast [KDL v2](https://kdl.dev) parser into a borrowed document tree with
a byte span on every node, name, type annotation, entry, property key,
value and children block. Its only dependency is `thiserror`. The optional
`kdl` feature converts to and from [`kdl::KdlDocument`](https://docs.rs/kdl).

```rust
let doc = kdl_tree::Document::parse(r#"server "api" port=8080"#).unwrap();
let port = &doc.nodes()[0].entries()[1];
assert_eq!(port.value(), &kdl_tree::Value::Integer(8080));
assert_eq!((port.span().offset, port.span().len), (13, 9));
```

The tree is for reading: comments, whitespace and the original spelling of
values are dropped. Use the `kdl` crate (or `kdl-pretty`) to edit or format.
Nesting is capped (`ParseOptions::max_depth`, 256 by default), so deep
input returns an error instead of overflowing the stack.

It parses 19× to 47× faster than `kdl`, depending on the input
(`benches/parse.rs`, results in
[BENCHMARKS.md](https://github.com/talaagent/kdl-kit/blob/main/BENCHMARKS.md)).

## Conformance

It passes all 338 cases of the official
[kdl-org/kdl test suite](https://github.com/kdl-org/kdl/tree/main/tests/test_cases)
(pinned in CI at `89c1087`), and a property test checks its values and spans
against the `kdl` crate. There are no known deviations from the KDL 2.0 spec.

The parser is derived from
[club-kdl](https://github.com/chronista-club/club-kdl) (MIT OR Apache-2.0,
see [NOTICE](https://github.com/talaagent/kdl-kit/blob/main/NOTICE)) and
differs from it in one grammar detail. club-kdl requires whitespace before
the `/-` of a slashdashed argument or property and rejects
`node "a"/-1` and `node "a"/-k=1`. kdl-tree accepts them. The KDL 2.0
grammar allows this (`node-space* (node-space | slashdash) node-prop-or-arg`),
and the suite's `zero_space_before_slashdash_arg` and
`zero_space_before_slashdash_prop` cases require it. A slashdashed children
block (`node "a"/-{}`) needs no whitespace in either parser.

## License

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-kit/blob/main/LICENSE).
