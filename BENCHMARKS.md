# Benchmarks

All numbers are divan benches, release profile, rustc 1.98.1, on a 32-thread
AMD Ryzen 9 9950X shared with other work. The box was heavily loaded both
times (load average 34–58 for kdl-serde, 110–118 for kdl-tree). Each side
ran twice and the table shows the lower of the two medians. Treat absolute
times as ±30% and small ratios (under 1.5×) as noise. The ratios are the
stable part.

## kdl-tree vs kdl: parsing

`kdl_tree::Document::parse` against `kdl::KdlDocument::parse_v2` (kdl
6.7.1) on the same text. Both turn UTF-8 into a document and nothing else.
Inputs are generated in `crates/kdl-tree/benches/parse.rs`.

```sh
nix develop -c cargo bench -p kdl-tree --bench parse -- --sample-count 10
```

Measured 2026-11-15.

| input | size | kdl | kdl-tree | speedup |
|---|---:|---:|---:|---:|
| `settings`: named blocks, properties, children, a multiline string | 1.0 MB | 367 ms (2.7 MB/s) | 19.2 ms (52 MB/s) | 19× |
| `siblings`: 20 000 `keyN N` nodes | 278 kB | 121 ms (2.3 MB/s) | 2.77 ms (100 MB/s) | 43× |
| `deep`: 100 levels of nesting | 32 kB | 2.21 ms (15 MB/s) | 46.8 µs (690 MB/s) | 47× |
| `strings`: escapes, raw, multiline and continued strings | 1.0 MB | 302 ms (3.3 MB/s) | 7.12 ms (140 MB/s) | 42× |
| `numbers`: decimal, hex, octal, binary, exponents, `#inf`/`#nan` | 1.0 MB | 280 ms (3.6 MB/s) | 9.73 ms (100 MB/s) | 28× |

kdl-tree parses 19× to 47× faster than kdl, depending on the input; the
settings-style document shows the smallest gain.

## kdl-serde: before and after the move to kdl-tree

kdl-serde used to read through the `kdl` crate's parser. These are the
benches in `crates/kdl-serde/benches/kdl.rs` with the `kdl` read path
(before) and the `kdl-tree` read path (after), measured 2026-11-12:

```sh
nix develop -c cargo bench -p kdl-serde --bench kdl -- --sample-count 10
```

| bench | input | before | after | speedup |
|---|---|---:|---:|---:|
| `from_str_big` | 1 MB settings doc → `Config` | 372 ms | 23.2 ms | 16× |
| `from_str_small_nodes` | 20 000 sibling nodes → `BTreeMap` | 117 ms | 5.70 ms | 20× |
| `from_str_deep` | 100 levels of nesting | 1.30 ms | 37.4 µs | 35× |
| `from_dir_500` | 500 files, 2 orgs each → `Config` | 119 ms | 36.1 ms | 3.3× |
| `to_string_pretty_big` | `Config` → 1 MB text (write path, unchanged) | 51.5 ms | 44.0 ms | ~1× |
| `parse_only_big` | parse the 1 MB doc (`kdl` before, `kdl-tree` after) | 308 ms | 6.61 ms | 47× |
| `load_dir_500` | `kdl_load::load_dir`, 500 files | 39.8 ms | 3.57 ms | 11× |
| `deserialize_only_big` | `from_loaded` on a parsed doc | 17.8 ms | 9.71 ms | 1.8× |
| `deserialize_only_small_nodes` | `from_loaded`, 20 000 siblings | 3.37 ms | 3.08 ms | ~1× |

In the same run, `parse_only_big_kdl` (the `kdl` crate on the same 1 MB
text) took 284 ms and `load_str_big` (`kdl_load::load_str`, parse plus
source map) 14.1 ms.

With kdl-tree, `from_str` time goes mostly to deserialization rather than
parsing. `from_dir_500` gains least because deserializing 1 000 orgs costs
more than loading the 500 files.
