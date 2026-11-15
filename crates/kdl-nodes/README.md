# kdl-nodes

Untyped helpers for the most common [KDL](https://kdl.dev) shape, named
blocks (`<kind> <name> { body }`), over
[`kdl-tree`](https://crates.io/crates/kdl-tree) documents: listing and
naming blocks, rejecting duplicate names, finding children and walking the
tree. No serde. For typed access, see
[`kdl-serde`](https://crates.io/crates/kdl-serde).

```rust
let loaded = kdl_load::load_str("schema.kdl", r#"
    struct User { field id "u64" }
    struct Post { field title "string" }
"#).unwrap();
let structs = kdl_nodes::named_blocks(loaded.tree(), "struct", loaded.sources()).unwrap();
let names: Vec<&str> = structs.iter().map(|b| b.name).collect();
assert_eq!(names, ["User", "Post"]);
assert_eq!(kdl_nodes::children(structs[0].node).count(), 1);
```

Errors carry stable codes (`kdl_nodes::duplicate_name`,
`kdl_nodes::expected_identifier`) and labeled spans; with the `miette`
feature they are `miette::Diagnostic`s.

API docs: [docs.rs/kdl-nodes](https://docs.rs/kdl-nodes).

## License

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-kit/blob/main/LICENSE).
