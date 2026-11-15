# kdl-pretty-cli

[![crates.io][version-badge]][crate] [![downloads][downloads-badge]][crate] [![lib.rs][librs-badge]][librs] [![License: Apache-2.0][license-badge]][license]

The `kdl-pretty` command: formats KDL v2 files with
[`kdl-pretty`](https://crates.io/crates/kdl-pretty). Comments are kept and
the output is deterministic.

## Install

```sh
cargo install kdl-pretty-cli
```

## Usage

```sh
kdl-pretty [--check] [--indent N] [FILES...]
```

- With files, it rewrites each file whose formatting changes in place.
  Files that are already formatted are left alone.
- With no files, or `-`, it reads stdin and writes the formatted document
  to stdout.
- `--check` writes nothing. It prints the files that would change to
  stderr and exits 1 if there are any.
- `--indent N` sets the spaces per level (default 2).

Exit codes: 0 success, 1 `--check` found files to change, 2 a parse or I/O
error. Parse errors are rendered with [miette](https://crates.io/crates/miette)
and point at the file and line. The other files are still formatted.

## treefmt

```toml
[formatter.kdl]
command = "kdl-pretty"
includes = ["*.kdl"]
```

## License

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-kit/blob/main/LICENSE).

[version-badge]: https://img.shields.io/crates/v/kdl-pretty-cli.svg
[downloads-badge]: https://img.shields.io/crates/d/kdl-pretty-cli.svg
[crate]: https://crates.io/crates/kdl-pretty-cli
[librs-badge]: https://img.shields.io/badge/lib.rs-kdl--pretty--cli-purple.svg
[librs]: https://lib.rs/crates/kdl-pretty-cli
[license-badge]: https://img.shields.io/badge/license-Apache--2.0-blue.svg
[license]: https://github.com/talaagent/kdl-kit/blob/main/LICENSE
