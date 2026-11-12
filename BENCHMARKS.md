# Benchmarks

The read path moved from the `kdl` crate's parser to `kdl-tree` in
`cfd67d5`. These are the divan benches in `crates/kdl-serde/benches/kdl.rs`
before (`8bda293`) and after (`cfd67d5` + this file's bench changes):

```sh
nix develop -c cargo bench -p kdl-serde --bench kdl -- --sample-count 10
```

Measured 2026-11-12, rustc 1.98.1, release profile, on a shared 32-thread
box under heavy load (load average 34–58). Each side ran twice, alternating;
the table shows the lower of the two medians. Treat absolute times as ±30%
and small ratios (under 1.5×) as noise.

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

New benches after the change: `parse_only_big_kdl` (the `kdl` crate on the
same 1 MB text, for comparison) 284 ms; `load_str_big` (`kdl_load::load_str`,
parse plus source map) 14.1 ms.

After the change, `from_str` time is mostly deserialization, not parsing.
`from_dir_500` gains least because deserializing 1 000 orgs costs more than
loading the 500 files.
