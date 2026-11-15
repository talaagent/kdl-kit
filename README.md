# kdl-kit

[![CI][ci-badge]][ci] [![crates.io][version-badge]][crate] [![downloads][downloads-badge]][crate] [![docs.rs][docs-badge]][docs] [![lib.rs][librs-badge]][librs] [![Rust 1.85+][msrv-badge]][msrv] [![License: Apache-2.0][license-badge]][license]

KDL v2 for Rust: a fast span-carrying parser, serde support, and a
deterministic formatter. It also loads files and whole directories as one
document with a source map, so errors point at the right file, line and
column.

```toml
[dependencies]
kdl-serde = "0.1"
serde = { version = "1", features = ["derive"] }
```

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    server: Vec<Server>,
}

#[derive(Deserialize)]
struct Server {
    #[serde(rename = "$0")]
    name: String,
    port: u16,
    #[serde(default)]
    tls: bool,
}

let config: Config = kdl_serde::from_str(r#"
    server "api" port=8080 { tls }
    server "admin" port=9000
"#)?;
assert_eq!(config.server[1].port, 9000);
assert!(config.server[0].tls);
# Ok::<(), kdl_serde::Error>(())
```

Errors carry a stable code, a message, help and spans that resolve to a
file, line and column. With the `miette` feature they render as
[miette](https://docs.rs/miette) diagnostics.

## Crates

| Crate | What it does |
| --- | --- |
| [`kdl-serde`](crates/kdl-serde) | serde `Deserializer` and `Serializer`: `from_str`, `from_dir`, `named_blocks`, `to_string_pretty`, … |
| [`kdl-tree`](crates/kdl-tree) | KDL v2 parser into a borrowed tree with byte spans on every part; passes the official test suite |
| [`kdl-span`](crates/kdl-span) | Source maps (many files in one offset space), spans, lazy line/col, labels, did-you-mean |
| [`kdl-load`](crates/kdl-load) | Load a string, file or directory of `*.kdl` files as one document with a source map |
| [`kdl-nodes`](crates/kdl-nodes) | Untyped helpers for `<kind> <name> { … }` blocks, identifiers, duplicate names, child walking |
| [`kdl-pretty`](crates/kdl-pretty) | Deterministic formatter that keeps comments |
| [`kdl-pretty-cli`](crates/kdl-pretty-cli) | The `kdl-pretty` command (`--check`, treefmt) |

Reading goes through `kdl-tree`, which parses 19× to 47× faster than the
[`kdl`](https://docs.rs/kdl) crate, depending on the input (see
[BENCHMARKS.md](BENCHMARKS.md)). Writing and formatting use `kdl` v6. Only
`kdl-serde` depends on serde. Every error code is listed in
[docs/errors.md](docs/errors.md). Used by korek.

## Features

| Feature | Crates | Default | Effect |
| --- | --- | --- | --- |
| `parallel` | kdl-load, kdl-serde | on | Read and parse directory files on rayon's thread pool |
| `miette` | kdl-span, kdl-load, kdl-nodes, kdl-serde | off | `miette::Diagnostic` impls for the error types |
| `kdl` | kdl-tree | off | Conversions to and from `kdl::KdlDocument` |

## MSRV

Each crate declares its own `rust-version`, and CI builds and tests each
crate on it.

| Crates | MSRV |
| --- | --- |
| kdl-tree, kdl-span, kdl-load, kdl-nodes | 1.85 (kdl-tree's `kdl` feature: 1.95) |
| kdl-serde, kdl-pretty, kdl-pretty-cli | 1.95, because the `kdl` crate needs it |

Raising an MSRV is a minor-version change.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE). The KDL parser in
`kdl-tree` is derived from club-kdl; see [NOTICE](NOTICE).

[ci-badge]: https://github.com/talaagent/kdl-kit/actions/workflows/ci.yml/badge.svg
[ci]: https://github.com/talaagent/kdl-kit/actions/workflows/ci.yml
[version-badge]: https://img.shields.io/crates/v/kdl-serde.svg
[downloads-badge]: https://img.shields.io/crates/d/kdl-serde.svg
[crate]: https://crates.io/crates/kdl-serde
[docs-badge]: https://docs.rs/kdl-serde/badge.svg
[docs]: https://docs.rs/kdl-serde
[librs-badge]: https://img.shields.io/badge/lib.rs-kdl--serde-purple.svg
[librs]: https://lib.rs/crates/kdl-serde
[msrv-badge]: https://img.shields.io/badge/rust-1.85%2B-orange.svg
[msrv]: https://github.com/talaagent/kdl-kit#msrv
[license-badge]: https://img.shields.io/badge/license-Apache--2.0-blue.svg
[license]: https://github.com/talaagent/kdl-kit/blob/main/LICENSE
