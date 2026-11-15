# kdl-span

Source maps, spans and diagnostic building blocks for
[KDL](https://kdl.dev) documents parsed with
[`kdl-tree`](https://crates.io/crates/kdl-tree).

A `SourceMap` gives each source file a `FileId` and its own range in one
global offset space, so a document merged from several files can keep plain
global spans. `SourceMap::resolve` turns a global span into a file-local
`Span { file, offset, len }`, and `SourceFile::line_col` computes line and
column only when needed. Also here: `Label`s for diagnostics and
`did_you_mean` suggestions.

```rust
use kdl_span::SourceMap;

let mut map = SourceMap::new();
let a = map.add("a.kdl", "node 1\n");
let b = map.add("b.kdl", "x\ny 2\n");
let base = map.file(b).base();
let span = map.span(base + 4, 1).unwrap();
assert_eq!(span.file, b);
assert_eq!(map.file(span.file).line_col(span.offset), (2, 3));
assert_eq!(map.file(a).name(), "a.kdl");
```

With the `miette` feature, `SourceFile` is a `miette::SourceCode` and
`Span` and `Label` convert to miette's span types.

API docs: [docs.rs/kdl-span](https://docs.rs/kdl-span).

## License

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-serde/blob/main/LICENSE).
