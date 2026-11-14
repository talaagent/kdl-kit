# kdl-serde

Generic [KDL v2] libraries, used by talaagent/korek for schemas and
configuration. Reading goes through our own parser and span-carrying tree
(`kdl-tree`, ~10× faster end to end than the [`kdl`] crate); writing and
formatting use [`kdl`] (v6). Depend on them via git:

```toml
kdl-serde = { git = "https://github.com/talaagent/kdl-serde" }
```

| Crate | What | Depends on |
| --- | --- | --- |
| [`kdl-tree`](crates/kdl-tree) | Fast KDL v2 parser (338/338 on the official suite) into a borrowed tree with byte spans on every part; depth-capped | thiserror (kdl, optional) |
| [`kdl-span`](crates/kdl-span) | Source maps (files in one offset space), `Span { file, offset, len }`, lazy line/col, labels, did-you-mean | kdl-tree |
| [`kdl-load`](crates/kdl-load) | Load a string, file or directory of `*.kdl` (sorted, recursive or not, parallel) as one `kdl_tree::Document` with a source map | kdl-tree, kdl-span (rayon) |
| [`kdl-nodes`](crates/kdl-nodes) | Untyped helpers: `<kind> <name> { … }` blocks, single identifiers, duplicate names, child walking | kdl-tree, kdl-span |
| [`kdl-pretty`](crates/kdl-pretty) | Deterministic formatter for any `KdlDocument`, and the `kdl-pretty` CLI (treefmt-friendly, `--check`) | kdl |
| [`kdl-serde`](crates/kdl-serde) | serde `Deserializer`/`Serializer` with KDL-shaped conventions; typed `from_dir`, `named_blocks`, `named_map` | serde + the above |

Only `kdl-serde` depends on serde (CI checks this). Every crate's errors are
`thiserror` types with stable codes ([docs/errors.md](docs/errors.md)), help
text and spans; the `miette` feature (off by default) adds
`miette::Diagnostic` impls.

## Development

```sh
nix develop -c cargo test --workspace --all-features
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop -c cargo bench -p kdl-serde
```

Before/after numbers for the move to `kdl-tree` are in
[BENCHMARKS.md](BENCHMARKS.md).

[KDL v2]: https://kdl.dev
[`kdl`]: https://docs.rs/kdl

## License

Apache-2.0. Copyright 2026 Ian Macalinao.
