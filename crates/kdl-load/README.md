# kdl-load

Load a [KDL](https://kdl.dev) string, file or directory of `*.kdl` files
as one [`kdl-tree`](https://crates.io/crates/kdl-tree) document, with a
[`kdl-span`](https://crates.io/crates/kdl-span) source map so every span
resolves to a file, line and column.

Files are merged in sorted path order. Each one is parsed straight into its
own range of the global offset space, so spans need no fixing up afterwards.
With the default `parallel` feature, files are read and parsed on rayon's
thread pool; the merge order stays the same.

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let dir = tempfile::tempdir()?;
# std::fs::write(dir.path().join("a.kdl"), "member ada")?;
# std::fs::write(dir.path().join("b.kdl"), "\nmember grace")?;
let loaded = kdl_load::load_dir(dir.path(), false)?;
let grace = &loaded.tree().nodes()[1];
let span = loaded.sources().node_name(grace).unwrap();
let file = loaded.sources().file(span.file);
assert!(file.name().ends_with("b.kdl"));
assert_eq!(file.line_col(span.offset), (2, 1));
# Ok(())
# }
```

Errors carry stable codes (`kdl_load::io`, `kdl_load::parse`); with the
`miette` feature they are `miette::Diagnostic`s.

API docs: [docs.rs/kdl-load](https://docs.rs/kdl-load).

## License

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-serde/blob/main/LICENSE).
