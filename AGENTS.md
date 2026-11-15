# kdl-kit: contributor and agent guide

KDL v2 libraries for Rust: kdl-tree, kdl-span, kdl-load, kdl-nodes,
kdl-pretty (+ kdl-pretty-cli) and kdl-serde, published together at one
version. Reading goes through kdl-tree's parser. The `kdl` crate (v6) is
used for writing and formatting, for kdl-tree's optional conversions, and
as a test oracle. Other projects depend on these crates, so public APIs stay
small, documented and stable. CONTRIBUTING.md covers the pull request flow
and releases.

## Commands

Use the nix dev shell. It pins rustc (see `rust-toolchain.toml`) and
provides cargo-nextest, cargo-deny and cargo-insta:

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop -c cargo nextest run --workspace --all-features
nix develop -c cargo test --workspace --all-features --doc
nix develop -c cargo test -p kdl-serde --no-default-features   # each crate
RUSTDOCFLAGS="-D warnings" nix develop -c cargo doc --workspace --no-deps --all-features
nix develop -c cargo deny check
nix develop -c cargo publish --workspace --dry-run --allow-dirty
nix develop -c cargo bench -p kdl-tree    # and -p kdl-serde (divan)
```

To run the official KDL test suite, point `KDL_SPEC_DIR` at a kdl-org/kdl
checkout. CI pins the commit in `.github/workflows/ci.yml`:

```sh
KDL_SPEC_DIR=/path/to/kdl nix develop -c cargo test -p kdl-tree --test spec -- --nocapture
```

Every change must pass fmt, clippy (with and without `--all-features`), the
tests (default features, `--all-features`, and each crate with
`--no-default-features`) and the doc build. Each crate's MSRV is its
`rust-version`: the workspace default is 1.85 (kdl-tree, kdl-span, kdl-load,
kdl-nodes), and the crates that depend on `kdl` (kdl-pretty, kdl-pretty-cli,
kdl-serde) override it with 1.95. Keep the 1.85 crates free of `kdl` and of
newer language features such as let chains. CI's `msrv` job builds and tests
each crate on its own value (kdl-tree is check-only there: its tests use
`kdl`).

## Layout

- `crates/kdl-tree`: the parser (`src/parse.rs`, derived from club-kdl,
  see NOTICE) and the borrowed document tree with spans on everything;
  `ParseOptions { base, max_depth }`; coded errors. `tests/spec.rs` runs the
  official suite; `tests/differential.rs` compares against `kdl`;
  `benches/parse.rs` times both parsers.
- `crates/kdl-span`: `SourceMap` (files in one global offset space),
  `Span`, `Label`, lazy line/col, `did_you_mean`, `Spanned`.
- `crates/kdl-load`: string/file/dir → one `kdl_tree::Document<'static>`
  parsed straight into global offsets (`ParseOptions::base`); parallel
  behind the default `parallel` feature.
- `crates/kdl-nodes`: untyped named-block helpers, generic over `Doc` /
  `NodeLike`. No serde.
- `crates/kdl-pretty`: the deterministic formatter. Don't use kdl-rs
  autoformat or `Display` for output; it collapses multiline strings
  (kdl-org/kdl-rs#77). Depends on kdl only.
- `crates/kdl-pretty-cli`: the `kdl-pretty` binary (treefmt, `--check`).
- `crates/kdl-serde/src/de.rs` (reads `kdl_tree` nodes): `BlockDe`
  (document / children block), `GroupDe` (same-named siblings = one field),
  `NodeDe` (one node), `ValueDe` (one argument or property), `ElemDe` (a
  sequence element whose type decides whether the node is one element or
  one per argument). `Fields` builds the struct/map view, including the
  `$0`, `$args`, `@prop`, `$name`, `$type`, `$children` conventions.
- `crates/kdl-serde/src/ser.rs`: serializes to an intermediate `Out`, then
  `Lower` turns it into nodes for a field name.
- `crates/kdl-serde/src/error.rs`: `Error` with stable codes; raw document
  offsets are resolved against the `SourceMap` at the public entry points.
- `docs/errors.md`: every error code (a test keeps it in sync).
- kdl-serde tests: `tests/mapping.rs` (one test per rule), `tests/errors.rs`
  (insta snapshots, `--features miette`), `tests/roundtrip.rs` (proptest,
  every `Options` style), `tests/fixtures.rs` + `tests/fixtures/*.kdl`,
  `tests/error_codes.rs`; benches in `benches/kdl.rs`.

## Conventions

- Every error a deserializer returns gets a span: wrap visitor calls with
  the deserializer's `wrap`/`at` so errors without a location pick up the
  current node's (innermost wins).
- Errors are thiserror types with a stable `code()` per kind. A new code
  needs a section in docs/errors.md: an anchor line
  `<a id="kdl_serde-unknown_field"></a>` and a heading
  ``## `kdl_serde::unknown_field` `` (`tests/error_codes.rs` checks both),
  plus an insta snapshot in `tests/errors.rs`. Review snapshot changes
  with `cargo insta review` and accept only intended diffs.
- miette only behind each crate's `miette` feature; no miette types in
  public APIs otherwise.
- Only kdl-serde may depend on serde (CI checks this).
- A mapping change needs a `tests/mapping.rs` case, a row in the README
  mapping table, and a round trip that still passes in every `Options`
  style (`tests/roundtrip.rs`).
- Crate READMEs are crate docs or doctests (`include_str!`), so their
  examples must compile and pass. Keep their links absolute so they work on
  crates.io.
- No `unwrap` outside tests; `core`/`alloc` paths over `std` where they
  exist (clippy enforces both).
- Changelog entries come from commit messages (release-plz), so write
  commit subjects that read well in CHANGELOG.md.
