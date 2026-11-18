# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/talaagent/kdl-kit/compare/v0.1.0...v0.1.1) - 2026-11-18

### Added

- *(kdl-load)* no_std + alloc, with file loading behind a default std feature ([#3](https://github.com/talaagent/kdl-kit/pull/3))
- no_std + alloc for kdl-tree, kdl-span and kdl-nodes ([#1](https://github.com/talaagent/kdl-kit/pull/1))

### Other

- Rust 1.100.0 ([#4](https://github.com/talaagent/kdl-kit/pull/4))

## [0.1.0](https://github.com/talaagent/kdl-kit/releases/tag/v0.1.0) - 2026-11-15

First release. All crates share one version.

### Added

- `kdl-tree`: a KDL v2 parser into a borrowed document tree with byte spans
  on every node, name, type annotation, entry, property key, value and
  children block. Passes the official kdl-org/kdl test suite; depth-capped;
  structured errors with stable codes; optional `kdl` conversions.
- `kdl-span`: source maps that place many files in one global offset space,
  spans, labels, lazy line and column, and did-you-mean suggestions.
- `kdl-load`: load a string, file or directory of `*.kdl` files as one
  document with a source map, in parallel by default.
- `kdl-nodes`: untyped helpers for `<kind> <name> { … }` blocks, identifiers,
  duplicate-name checks and child walking.
- `kdl-pretty`: a deterministic, comment-preserving KDL v2 formatter.
- `kdl-pretty-cli`: the `kdl-pretty` command, with `--check` for CI and
  treefmt.
- `kdl-serde`: serde `Deserializer` and `Serializer` with KDL-shaped
  conventions (`$0`, `$args`, `@prop`, `$name`, `$type`, `$children`),
  `from_dir`, `named_blocks` and `named_map`, and errors with stable codes,
  spans and help.
- `miette` feature on kdl-span, kdl-load, kdl-nodes and kdl-serde for
  `miette::Diagnostic` impls.
- MSRV per crate: Rust 1.85 for kdl-tree (1.95 with its `kdl` feature),
  kdl-span, kdl-load and kdl-nodes; 1.95 for kdl-pretty, kdl-pretty-cli and
  kdl-serde.
