# kdl-serde

A [serde] `Deserializer` and `Serializer` for [KDL v2] documents, built on the
[`kdl`] crate (v6). Errors are [miette] diagnostics that point at the file,
line and column of the offending node, argument or property.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    port: u16,
    tags: Vec<String>,
}

let text = r#"
    name "api"
    port 8080
    tags "web" "public"
"#;
let config: Config = kdl_serde::from_str(text)?;
assert_eq!(config.port, 8080);
assert_eq!(kdl_serde::to_string_pretty(&config)?, "name \"api\"\nport 8080\ntags \"web\" \"public\"\n");
# Ok::<(), kdl_serde::Error>(())
```

[serde]: https://serde.rs
[KDL v2]: https://kdl.dev
[`kdl`]: https://docs.rs/kdl
[miette]: https://docs.rs/miette

## API

| Function | |
| --- | --- |
| `from_str`, `from_str_named` | Text (optionally with a file name for errors) to `T` |
| `from_reader`, `from_file` | A reader or a file to `T` |
| `from_doc` | A parsed `kdl::KdlDocument` to `T` |
| `from_dir(path, recursive)` | Every `*.kdl` file in a directory, sorted by path, read as one document |
| `to_string_pretty`, `to_string_with` | `T` to indented text (default or custom `Options`) |
| `to_string` | `T` to compact text, one top-level node per line |
| `to_doc`, `to_doc_with` | `T` to a `kdl::KdlDocument` |
| `format`, `format_compact` | Print any `KdlDocument` with the deterministic printer |

Helper types: `Annotated<T>` (a value and its `(type)` annotation), `RawNode`
and `RawDocument` (pass a node or a children block through untouched).

## Mapping

A document, or a node's children block, is a struct whose fields are child
nodes. The field-name conventions in the second table reach a node's
arguments, properties, name and annotation; use `#[serde(rename = "…")]` to
apply them.

| Rust | KDL (written) | Also read |
| --- | --- | --- |
| struct (document or node body) | one child node per field | |
| scalar field `name: T` | `name "value"` | a property `name="value"` on the parent node |
| `bool` | `debug #true` | a bare `debug` is `true` |
| integers, `f32`/`f64` | `port 8080`, `ratio 0.5`, `#inf`, `#-inf`, `#nan` | integers for floats |
| `char` | `sep ","` | |
| `String` with newlines | `"""` multiline string, indented | any string form |
| `Option<T>` | omitted when `None` | `field #null` |
| `()`, unit struct | `field` (empty node) | `field #null` |
| bytes (`serialize_bytes`) | `data (base64)"aGk="` | base64 with or without the annotation |
| `Vec<scalar>` | `tags "a" "b"` (`SeqStyle::Args`) or `tags "a"; tags "b"` (`SeqStyle::Repeated`) | any mix of the two; `tags { - "a"; - "b" }` |
| `Vec<struct>` | `member { … }` repeated | `member { - { … } }` |
| empty `Vec` | `tags` | |
| tuple, tuple struct | `point 1 2` | |
| `Vec<Vec<T>>` | `row 1 2; row 3` | |
| `Map<K, scalar>` | `env "HOME" "/root"` repeated (`MapStyle::Keyed`) | `env { HOME "/root" }`, `env HOME="/root"` |
| `Map<K, struct>` | `site "eu" { … }` repeated | `site { eu { … } }` |
| `Map` (`MapStyle::Block`) | `env { HOME "/root" }` | |
| top-level `Map<K, V>` | one node per key | |
| top-level `Vec<T>` | one node per element: `-`, or the variant name for enums | |
| unit variant | `mode "fast"` | |
| newtype variant | `shape (circle)1.5` | `shape { circle 1.5 }` |
| tuple variant | `(line)shape 0 3` | `shape { line 0 3 }` |
| struct variant | `(rect)shape { w 1; h 2 }` | `(rect)shape w=1 h=2`, `shape { rect { … } }` |
| enum in `$children` or a top-level `Vec` | `circle 1.5`, `rect { … }` (node name = variant) | |
| `#[serde(tag = "type")]` | (see limitations) | `shape type="circle" r=1`, `shape { type "circle" }` |
| `#[serde(untagged)]` | as the matching variant | |
| `Annotated<T>` | `size (u8)5`, `(point)pos { … }` | |
| `RawNode` / `RawDocument` | the node / the children as they were | |

| Field name | Means |
| --- | --- |
| `$0`, `$1`, … | positional argument 0, 1, … |
| `$args` | the positional arguments no `$N` field took (a sequence) |
| `@key` | property `key=…` (write side; plain fields also *read* properties) |
| `$name` | the node's name |
| `$type` | the node's `(type)` annotation |
| `$children` | the child nodes no other field took (a sequence or map) |

### Named nodes: `$0`

A struct with a `$0` field reads nodes like `member "ada" { … }`. A `Vec`
of them reads repeated nodes; a map keyed by the first argument is the
alternative when the name is unique.

```rust
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Org {
    member: Vec<Member>,
    site: BTreeMap<String, Site>,
}

#[derive(Deserialize)]
struct Member {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "@role")]
    role: String,
    #[serde(default)]
    email: Vec<String>,
}

#[derive(Deserialize)]
struct Site {
    region: String,
}

let org: Org = kdl_serde::from_str(r#"
    member ada role="owner" { email "ada@example.com" }
    member grace role="admin"
    site eu { region "eu-west-1" }
    site us { region "us-east-1" }
"#)?;
assert_eq!(org.member[1].name, "grace");
assert_eq!(org.site["us"].region, "us-east-1");
# Ok::<(), kdl_serde::Error>(())
```

### Arguments and properties

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Run {
    #[serde(rename = "$0")]
    program: String,
    #[serde(rename = "$args")]
    args: Vec<String>,
    #[serde(rename = "@cwd")]
    cwd: String,
    #[serde(default)]
    env: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Task {
    run: Run,
}

let task: Task = kdl_serde::from_str(r#"run "ls" "-l" "/" cwd="/tmp""#)?;
assert_eq!(task.run.args, ["-l", "/"]);
assert_eq!(kdl_serde::to_string_pretty(&task)?, "run \"ls\" \"-l\" \"/\" cwd=\"/tmp\" {\n  env\n}\n");
# Ok::<(), kdl_serde::Error>(())
```

### Enums

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Shape {
    Point,
    Circle(f64),
    Rect { w: f64, h: f64 },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Drawing {
    shape: Vec<Shape>,
}

let d = Drawing { shape: vec![Shape::Point, Shape::Circle(1.5), Shape::Rect { w: 1.0, h: 2.0 }] };
let text = kdl_serde::to_string_pretty(&d)?;
assert_eq!(text, "shape \"point\"\nshape (circle)1.5\n(rect)shape {\n  w 1.0\n  h 2.0\n}\n");
assert_eq!(kdl_serde::from_str::<Drawing>(&text)?, d);

// A list of variants as nodes named by the variant.
#[derive(Debug, PartialEq, Deserialize)]
struct Canvas {
    #[serde(rename = "$children")]
    shapes: Vec<Shape>,
}
let c: Canvas = kdl_serde::from_str("point\ncircle 2.0\nrect w=1.0 h=1.0")?;
assert_eq!(c.shapes[1], Shape::Circle(2.0));
# Ok::<(), kdl_serde::Error>(())
```

### Type annotations

```rust
use kdl_serde::Annotated;
use serde::Deserialize;

#[derive(Deserialize)]
struct Limits {
    max_upload: Annotated<u64>,
}

let l: Limits = kdl_serde::from_str("max_upload (bytes)1048576")?;
assert_eq!(l.max_upload.ty.as_deref(), Some("bytes"));
assert_eq!(l.max_upload.value, 1_048_576);
# Ok::<(), kdl_serde::Error>(())
```

### Directories

`from_dir` reads `conf.d/`-style directories: every `*.kdl` file, sorted by
path, as one document, so `member` nodes spread over several files collect
into one `Vec`. Errors name the file they come from.

### Errors

Every deserialization error carries the source and a labeled span:

```text
  × unknown property `rol`, expected `role` at config.kdl:3:27
   ╭─[config.kdl:3:27]
 2 │ server host="h" port=1
 3 │ member "ada" role="admin" rol="x"
   ·                           ───┬───
   ·                              ╰── unknown property
   ╰────
  help: did you mean `role`?
```

`Error::kind()` tells missing fields, unknown fields (under
`deny_unknown_fields`), duplicate nodes for single-valued fields, wrong types,
out-of-range values and parse errors apart; `Error::file()` and
`Error::line_col()` give the location.

### Printing

`to_string_pretty` uses its own printer rather than kdl-rs's autoformat: it
ignores source formatting, indents two spaces (`Options::indent`), keeps
struct field order, sorts map entries (`Options::sort_maps`), writes
identifiers bare when KDL allows it, and writes strings containing newlines
as `"""` multiline strings (kdl-rs collapses them to `\n` escapes,
kdl-org/kdl-rs#77). The output is stable: printing what it reads back gives
the same text.

## Limitations

- Buffered serde features (`untagged`, internally tagged enums, `flatten`)
  see a node through `deserialize_any`: a single argument is a scalar, only
  arguments are a sequence, anything else is a map of `$0`, `$1`, … for
  arguments, property names, and child names. `@key` fields do not match
  there (use plain names), and a `Vec` field holding a single node reads as
  that node, not a one-element list. Internally tagged enums are read, but
  written as externally tagged.
- `u128` values above `i128::MAX` cannot be written. kdl 6.7 cannot parse
  `-170141183460469231731687303715884105728` (`i128::MIN`).
- Comments and formatting are not preserved; `RawNode` and `RawDocument`
  keep content, not layout.
- `Option<Option<T>>` cannot tell `Some(None)` from `None`.

## Prior art

[knuffel] and [facet-kdl] map KDL through their own derive macros with
per-field attributes (argument, property, child, children); kdl-rs 6.7's
built-in serde support, serde-kdl2 and serde_kdl (KDL v1) treat a document
like JSON (single-argument nodes are scalars, multi-argument nodes are
sequences, repeated names are sequences). kdl-serde stays with plain serde
derives but adds field-name conventions so the same struct can read
`node "arg" key=value { child … }`, and accepts the common spellings of
lists, maps and variants on read.

[knuffel]: https://docs.rs/knuffel
[facet-kdl]: https://docs.rs/facet-kdl

## License

Apache-2.0. Copyright 2026 Ian Macalinao.
