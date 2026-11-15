# kdl-load

[![crates.io][version-badge]][crate] [![downloads][downloads-badge]][crate] [![docs.rs][docs-badge]][docs] [![lib.rs][librs-badge]][librs] [![License: Apache-2.0][license-badge]][license]

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

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-kit/blob/main/LICENSE).

[version-badge]: https://img.shields.io/crates/v/kdl-load.svg
[downloads-badge]: https://img.shields.io/crates/d/kdl-load.svg
[crate]: https://crates.io/crates/kdl-load
[docs-badge]: https://docs.rs/kdl-load/badge.svg
[docs]: https://docs.rs/kdl-load
[librs-badge]: https://img.shields.io/badge/lib.rs-kdl--load-purple.svg
[librs]: https://lib.rs/crates/kdl-load
[license-badge]: https://img.shields.io/badge/license-Apache--2.0-blue.svg
[license]: https://github.com/talaagent/kdl-kit/blob/main/LICENSE
