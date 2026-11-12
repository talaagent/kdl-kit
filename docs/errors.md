# Error codes

Every error in the kdl-serde workspace has a stable code (`Error::code()`,
and the miette diagnostic code with the `miette` feature). Codes are
never reused; a renamed code stays listed here as retired.

<a id="kdl_load-io"></a>

## `kdl_load::io`

A file or directory could not be read. The path does not exist, is not readable, or a directory entry vanished while it was listed. The error's source is the underlying I/O error.

```rust
kdl_serde::from_file::<Config>("missing.kdl")
```

<a id="kdl_load-parse"></a>

## `kdl_load::parse`

The input is not valid KDL v2. The label points at the syntax error; `kdl_load::ParseError::kind` (and `ParseProblem::kind`) says which one, with its own `kdl_tree::*` code below. KDL v1 syntax (bare `true`, `null`, `r"raw"` strings) is a common cause.

```kdl
host "unterminated
```

<a id="kdl_tree-unexpected"></a>

## `kdl_tree::unexpected`

The grammar does not allow what is here: a missing node name, value, `)` or node terminator, an entry with no whitespace before it, or a stray `}`. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
node "a""b"
```

<a id="kdl_tree-unterminated"></a>

## `kdl_tree::unterminated`

A string, block comment or children block runs to the end of the input. The label points at where it starts. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
host "localhost
```

<a id="kdl_tree-disallowed_char"></a>

## `kdl_tree::disallowed_char`

A code point KDL forbids anywhere: control characters other than whitespace, bidi overrides, or a byte-order mark after the start. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```text
node "a<U+0007>b"
```

<a id="kdl_tree-invalid_number"></a>

## `kdl_tree::invalid_number`

A number is malformed (`0x` with no digits, `1.2.3`, letters after the digits) or does not fit in an `i128`. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
port 0x
```

<a id="kdl_tree-invalid_identifier"></a>

## `kdl_tree::invalid_identifier`

A bare identifier is a KDL keyword (`true`, `false`, `null`, `inf`, `-inf`, `nan`) or looks like a number (`.5`). Write `#true` for the keyword, or quote the string. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
enabled true
```

<a id="kdl_tree-invalid_keyword"></a>

## `kdl_tree::invalid_keyword`

A `#` keyword that KDL does not have; the keywords are `#true`, `#false`, `#null`, `#inf`, `#-inf` and `#nan`. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
enabled #yes
```

<a id="kdl_tree-invalid_string"></a>

## `kdl_tree::invalid_string`

A string is malformed: a newline in a single-line string, a multi-line string without a newline after its opening `"""`, or a line that does not start with the closing line's indentation. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
text """
  one
 two
  """
```

<a id="kdl_tree-invalid_escape"></a>

## `kdl_tree::invalid_escape`

An unknown escape (`\q`) or a malformed `\u{…}` in a quoted string. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
path "C:\Users"
```

<a id="kdl_tree-children_order"></a>

## `kdl_tree::children_order`

An argument or property after a node's children block, or a second children block. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```kdl
node { child } extra=1
```

<a id="kdl_tree-too_deep"></a>

## `kdl_tree::too_deep`

Children blocks are nested deeper than the parser's limit (`kdl_tree::ParseOptions::max_depth`, 256 by default), which keeps hostile input from overflowing the stack. It is reported as [`kdl_load::parse`](#kdl_load-parse) with this kind.

```text
a { a { a { … 257 levels … } } }
```

<a id="kdl_nodes-duplicate_name"></a>

## `kdl_nodes::duplicate_name`

Two named blocks of one kind share a name. `named_blocks` and `named_map` need `<kind> <name>` to be unique. Both definitions are labeled (or, across files, the first is named in the help).

```kdl
struct User { }
struct User { }
```

<a id="kdl_nodes-expected_identifier"></a>

## `kdl_nodes::expected_identifier`

A named block has no single string name. `<kind> <name> { … }` needs exactly one positional string argument; properties and children are fine.

```kdl
struct { field id "u64" }
```

<a id="kdl_serde-missing_field"></a>

## `kdl_serde::missing_field`

A required struct field, argument or property is absent. Fields map to child nodes, `$N` fields to arguments and `@key` fields to properties. Make the field `Option<T>` or add `#[serde(default)]` if it may be left out.

```kdl
server { host "h" }   // no `port`
```

<a id="kdl_serde-unknown_field"></a>

## `kdl_serde::unknown_field`

A node, argument or property matches no field. Only raised under `#[serde(deny_unknown_fields)]` (or for arguments when the struct has no `$N` field and denies unknown fields). The help suggests the closest field name.

```kdl
prot 8080   // meant `port`
```

<a id="kdl_serde-duplicate_field"></a>

## `kdl_serde::duplicate_field`

A field is given both as a property and as a child node. Plain fields read either `name=value` on the node or a `name` child; giving both is ambiguous.

```kdl
server host="a" { host "b" }
```

<a id="kdl_serde-duplicate_node"></a>

## `kdl_serde::duplicate_node`

A single-valued field is given by more than one node. Only `Vec`s, maps and other sequence types take repeated nodes. Remove one, or change the field's type.

```kdl
port 1
port 2
```

<a id="kdl_serde-unknown_variant"></a>

## `kdl_serde::unknown_variant`

An enum variant name is not one of the enum's variants. Variants are named by a string value (`mode "fast"`), a type annotation (`shape (circle)1.0`, `(rect)shape { … }`) or a node name. The help suggests the closest variant.

```kdl
role "admn"
```

<a id="kdl_serde-invalid_type"></a>

## `kdl_serde::invalid_type`

A value has the wrong type. For example a string where a number is expected, several arguments for a single-valued field, a children block where a value is expected, or a node with no value.

```kdl
port "eighty"
```

<a id="kdl_serde-invalid_value"></a>

## `kdl_serde::invalid_value`

A value has the right type but is not accepted. Out-of-range integers (`port 70000` into a `u16`), malformed base64 bytes, or a value a `Deserialize` impl rejects with `invalid_value`.

```kdl
port 70000
```

<a id="kdl_serde-invalid_length"></a>

## `kdl_serde::invalid_length`

A tuple or fixed-size sequence has the wrong number of elements. Tuples and tuple structs are written as one node's arguments: `point 1 2`.

```kdl
point 1   // (i32, i32)
```

<a id="kdl_serde-unsupported"></a>

## `kdl_serde::unsupported`

The value cannot be written as KDL. Serializer only: `u128` values above `i128::MAX`, non-scalar map keys or properties, `$N` argument gaps, arguments or properties on the document itself.

```rust
kdl_serde::to_string(&u128::MAX)
```

<a id="kdl_serde-custom"></a>

## `kdl_serde::custom`

An error raised by a `Deserialize` or `Serialize` impl (`Error::custom`). The message is the impl's own; deserialization errors still point at the node being read.

```kdl
when "yesterday"   // a Deserialize impl rejecting the date
```
