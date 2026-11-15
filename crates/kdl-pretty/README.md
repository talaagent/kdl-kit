# kdl-pretty

[![crates.io][version-badge]][crate] [![downloads][downloads-badge]][crate] [![docs.rs][docs-badge]][docs] [![lib.rs][librs-badge]][librs] [![License: Apache-2.0][license-badge]][license]

A deterministic, comment-preserving formatter for KDL v2 documents, built
on the [`kdl`](https://crates.io/crates/kdl) crate (v6), its only
dependency.

```rust
let src = "server   \"main\" port=8080 { host localhost } // prod\n";
let out = kdl_pretty::format_str(src, &kdl_pretty::Options::default()).unwrap();
assert_eq!(out, "server \"main\" port=8080 {\n  host \"localhost\"\n} // prod\n");
```

API:

- `format(&KdlDocument, &Options) -> String`: one node per line, children
  indented by `Options::indent` spaces (default 2).
- `format_str(&str, &Options) -> Result<String, KdlError>`: parse as KDL v2,
  then `format`. Prefer this for files: it also restores comments that
  kdl-rs drops after a `}` (see below).
- `format_compact(&KdlDocument) -> String`: each top-level node on one
  line (`a 1 { b 2; c 3; }`), comments dropped.
- `is_bare_identifier(&str) -> bool`: whether a string can be written
  without quotes.

## Output

Values and identifiers are re-rendered from their parsed values, never
copied from the source: bare identifiers for names where KDL allows them,
quoted strings for values, decimal numbers (`0xff` → `255`), `#true`,
`#null`, … Strings containing newlines become `"""` multiline strings
indented one level past their node (`Options::multiline_strings`).
Strings written as raw strings (`#"…"#`, `#"""…"""#`) stay raw, with as
many `#`s as their content needs; a raw multiline string falls back to an
escaped one only if raw can't hold it (a whitespace-only line). Empty
children blocks are kept as `{}`.

Formatting is idempotent (`format_str(format_str(x)) == format_str(x)`)
and never changes the document's meaning.

### Comments

Kept, in normalized positions:

- `//` and `/* */` comments on their own lines before a node, at the
  node's indentation; a `/* */` directly before a node on the same line
  stays there.
- A `// comment` after a node (including after its `}`) stays on that
  line, as does one after a `{`.
- Comments at the end of a children block, before its `}`, and at the end
  of the document.
- `/* */` comments between entries, inside type annotations and after a
  `}`, kept inline; a `\ // comment` line continuation keeps its comment.
- Slashdashed nodes, entries and children blocks (`/-node`, `/-1`,
  `/-key=v`, `/-{ … }`), themselves formatted.
- Blank lines between nodes: a run of one or more becomes exactly one.
  Blank lines at the start or end of a block are removed.

Not kept: the position of a comment within a line beyond the rules above
(e.g. `a; /* x */ b` puts `/* x */` after `a` and `b` on the next line),
blank lines inside a node, and whitespace in general. A `\` line
continuation without a comment is joined into one line. `format` on an
already-parsed document can't see comments kdl-rs dropped after a `}`;
`format_str` can.

## CLI

The [`kdl-pretty-cli`](https://crates.io/crates/kdl-pretty-cli) package
installs a `kdl-pretty` binary:

```sh
cargo install kdl-pretty-cli
kdl-pretty [--check] [--indent N] [FILES...]
```

- With files: rewrites each file whose formatting changes, in place;
  files already formatted are not touched.
- With no files, or `-`: reads stdin and writes the formatted document
  to stdout.
- `--check`: writes nothing; prints the names of files that would change
  to stderr and exits 1 if any would (stdin: exits 1 if it would change).
- `--indent N`: spaces per level (default 2).
- Parse errors are rendered with miette, pointing at the file and line;
  the exit code is then 2 (other files are still formatted).

### treefmt

```toml
[formatter.kdl]
command = "kdl-pretty"
includes = ["*.kdl"]
```

## License

Apache-2.0. See [LICENSE](https://github.com/talaagent/kdl-kit/blob/main/LICENSE).

[version-badge]: https://img.shields.io/crates/v/kdl-pretty.svg
[downloads-badge]: https://img.shields.io/crates/d/kdl-pretty.svg
[crate]: https://crates.io/crates/kdl-pretty
[docs-badge]: https://docs.rs/kdl-pretty/badge.svg
[docs]: https://docs.rs/kdl-pretty
[librs-badge]: https://img.shields.io/badge/lib.rs-kdl--pretty-purple.svg
[librs]: https://lib.rs/crates/kdl-pretty
[license-badge]: https://img.shields.io/badge/license-Apache--2.0-blue.svg
[license]: https://github.com/talaagent/kdl-kit/blob/main/LICENSE
