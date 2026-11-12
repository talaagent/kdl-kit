# kdl-serde

Serde `Deserializer`/`Serializer` for KDL v2 on the `kdl` crate (v6), with
miette diagnostics. Consumed by talaagent/korek for schemas and config.

## Commands

Run everything inside the nix devShell (pinned rustc, see
`rust-toolchain.toml`):

```sh
nix develop -c cargo fmt
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix develop -c cargo insta review   # after changing error or fixture output
```

Every commit must pass fmt, clippy and test. Commit straight to `main`,
small and focused; never force-push. Not published to crates.io.

## Layout

- `src/de.rs`: the deserializer. `BlockDe` (document / children block),
  `GroupDe` (same-named siblings = one struct field), `NodeDe` (one node),
  `ValueDe` (one argument or property), `ElemDe` (a sequence element whose
  type decides whether the node is one element or one per argument).
  `Fields` builds the struct/map view of a node, including the `$0`,
  `$args`, `@prop`, `$name`, `$type`, `$children` conventions.
- `src/ser.rs`: serializes to an intermediate `Out`, then `Lower` turns it
  into nodes for a field name. `Options` (`SeqStyle`, `MapStyle`, …).
- `src/fmt.rs`: the deterministic printer behind `to_string_pretty`.
  Don't use kdl-rs autoformat or `Display` for output (it collapses
  multiline strings, kdl-org/kdl-rs#77).
- `src/error.rs`: `Error` (a miette `Diagnostic`) and its serde `Error`
  impls, which word messages for KDL (arguments, properties, did-you-mean).
- `src/types.rs`: `Annotated<T>`, `RawNode`, `RawDocument`; they talk to
  the (de)serializer through the magic names in `src/magic.rs`.
- `tests/mapping.rs` (one test per rule), `tests/errors.rs` (insta
  snapshots of rendered diagnostics), `tests/roundtrip.rs` (proptest, every
  `Options` style), `tests/fixtures.rs` + `tests/fixtures/*.kdl`.

## Conventions

- Every error a deserializer returns gets a span: wrap visitor calls with
  the deserializer's `wrap`/`at` so errors without a location pick up the
  current node's (innermost wins).
- A mapping change needs: a `tests/mapping.rs` case, the README table row,
  and a round-trip that still passes in every style.
- The README is the crate docs (`#![doc = include_str!("../README.md")]`),
  so its examples are doctests.
- No `unwrap` outside tests; `core`/`alloc` paths over `std` where they
  exist (clippy enforces both).
