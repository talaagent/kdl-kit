# kdl-serde workspace

Generic KDL v2 libraries on the `kdl` crate (v6): kdl-span, kdl-load,
kdl-nodes, kdl-pretty (+ kdl-pretty-cli) and kdl-serde. Consumed by
talaagent/korek via git, so public APIs stay small, documented and stable.

## Commands

Run everything inside the nix devShell (pinned rustc, see
`rust-toolchain.toml`):

```sh
nix develop -c cargo fmt
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop -c cargo test --workspace            # and --all-features (miette)
nix develop -c cargo bench -p kdl-serde         # divan benchmar## Layout

- `crates/kdl-span`: `SourceMap` (files in one global offset space),
  `Span`, `Label`, lazy line/col, `did_you_mean`. kdl only.
- `crates/kdl-load`: string/file/dir → one `KdlDocument` with spans shifted
  into the source map; parallel behind the default `parallel` feature.
- `crates/kdl-nodes`: untyped named-block helpers. No serde.
- `crates/kdl-pretty`: the deterministic formatter (don't use kdl-rs
  autoformat/`Display` for output; it collapses multiline strings,
  kdl-org/kdl-rs#77). kdl only.
- `crates/kdl-serde/src/de.rs`: `BlockDe` (document / children block),
  `GroupDe` (same-named siblings = one field), `NodeDe` (one node),
  `ValueDe` (one argument or property), `ElemDe` (a sequence element whose
  type decides whether the node is one element or one per argument).
  `Fields` builds the struct/map view, including the `$0`, `$args`,
  `@prop`, `$name`, `$type`, `$children` conventions.
- `crates/kdl-serde/src/ser.rs`: serializes to an intermediate `Out`, then
  `Lower` turns it into nodes for a field name.
- `crates/kdl-serde/src/error.rs`: `Error` with stable codes; raw document
  offsets are resolved against the `SourceMap` at the public entry points.
- `docs/errors.md`: every error code (a test keeps it in sync).

c/magic.rs`.
- `tests/mapping.rs` (one test per rule), `tests/errors.rs` (insta
  snapshots of rendered diagnostics), `tests/roundtrip.rs` (proptest, every
  `Options` style), `tests/fixtures.rs` + `tests/fixtures/*.kdl`.

## Conventions

- Every error a deserializer returns gets a span: wrap visitor calls with
  the deserializer's `wrap`/`at` so errors without a location pick up the
  current node's (innermost wins).
- Errors: thiserror types, a stable `code()` per kind (add new codes to
  docs/errors.md with an anchor and an insta snapshot); miette only behind
  each crate's `miette` feature; no miette types in public APIs otherwise.
- Only kdl-serde may depend on serde.
- A mapping change needs: a `tests/mapping.rs` case, the README table row,
  and a round-trip that still passes in every style.
- The README is the crate docs (`#![doc = include_str!("../README.md")]`),
  so its examples are doctests.
- No `unwrap` outside tests; `core`/`alloc` paths over `std` where they
  exist (clippy enforces both).
