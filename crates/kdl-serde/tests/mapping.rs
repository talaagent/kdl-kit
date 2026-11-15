//! One test (or a few) per mapping rule, reading and writing.

#![allow(clippy::unwrap_used, clippy::std_instead_of_core)]
// The float values here are exact by construction.
#![allow(clippy::float_cmp, clippy::items_after_statements)]

extern crate alloc;

use alloc::collections::BTreeMap;
use std::collections::HashMap;

use kdl_serde::{Annotated, MapStyle, Options, RawDocument, RawNode, SeqStyle};
use serde::{Deserialize, Serialize};

fn de<T: serde::de::DeserializeOwned>(s: &str) -> T {
    match kdl_serde::from_str(s) {
        Ok(v) => v,
        Err(e) => panic!("{e:?}"),
    }
}

fn ser<T: Serialize>(v: &T) -> String {
    kdl_serde::to_string_pretty(v).unwrap()
}

/// Writes `v`, checks the text, and reads it back.
fn round<T>(v: &T, expected: &str)
where
    T: Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
{
    let text = ser(v);
    assert_eq!(text, expected);
    let back: T = de(&text);
    assert_eq!(&back, v);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
    debug: bool,
}

#[test]
fn struct_fields_are_nodes() {
    round(
        &Server {
            host: "localhost".into(),
            port: 8080,
            debug: false,
        },
        "host \"localhost\"\nport 8080\ndebug #false\n",
    );
}

#[test]
fn bare_flag_node_is_true() {
    let s: Server = de("host localhost; port 1; debug");
    assert!(s.debug);
}

#[test]
fn plain_fields_also_read_properties() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Doc {
        server: Server,
    }
    let d: Doc = de(r#"server host="h" port=1 { debug #true }"#);
    assert_eq!(d.server.host, "h");
    assert_eq!(d.server.port, 1);
    assert!(d.server.debug);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Tags {
    tags: Vec<String>,
}

#[test]
fn scalar_vec_as_arguments() {
    round(
        &Tags {
            tags: vec!["a".into(), "b".into()],
        },
        "tags \"a\" \"b\"\n",
    );
}

#[test]
fn scalar_vec_as_repeated_nodes() {
    let t: Tags = de("tags a; tags b c");
    assert_eq!(t.tags, ["a", "b", "c"]);
    let opts = Options::default().seq_style(SeqStyle::Repeated);
    let v = Tags {
        tags: vec!["a".into(), "b".into()],
    };
    assert_eq!(
        kdl_serde::to_string_with(&v, &opts).unwrap(),
        "tags \"a\"\ntags \"b\"\n"
    );
}

#[test]
fn scalar_vec_as_dash_children() {
    let t: Tags = de("tags { - a; - b }");
    assert_eq!(t.tags, ["a", "b"]);
}

#[test]
fn empty_vec() {
    round(&Tags { tags: vec![] }, "tags\n");
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Member {
    #[serde(rename = "$0")]
    name: String,
    #[serde(rename = "@role", default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    email: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Team {
    member: Vec<Member>,
}

#[test]
fn named_node_collection() {
    round(
        &Team {
            member: vec![
                Member {
                    name: "ada".into(),
                    role: Some("admin".into()),
                    email: vec!["ada@example.com".into()],
                },
                Member {
                    name: "grace".into(),
                    role: None,
                    email: vec![],
                },
            ],
        },
        "member \"ada\" role=\"admin\" {\n  email \"ada@example.com\"\n}\nmember \"grace\"\n",
    );
}

#[test]
fn single_struct_element_is_one_element() {
    let t: Team = de("member ada");
    assert_eq!(t.member.len(), 1);
    assert_eq!(t.member[0].name, "ada");
}

#[test]
fn args_rest() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Cmd {
        #[serde(rename = "$0")]
        program: String,
        #[serde(rename = "$args")]
        args: Vec<String>,
    }
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        run: Cmd,
    }
    round(
        &Doc {
            run: Cmd {
                program: "ls".into(),
                args: vec!["-l".into(), "/".into()],
            },
        },
        "run \"ls\" \"-l\" \"/\"\n",
    );
}

#[test]
fn node_name_and_type() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Item {
        #[serde(rename = "$name")]
        kind: String,
        #[serde(rename = "$type", default, skip_serializing_if = "Option::is_none")]
        ty: Option<String>,
        #[serde(rename = "$0")]
        value: i64,
    }
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        #[serde(rename = "$children")]
        items: Vec<Item>,
    }
    round(
        &Doc {
            items: vec![
                Item {
                    kind: "a".into(),
                    ty: None,
                    value: 1,
                },
                Item {
                    kind: "b".into(),
                    ty: Some("u8".into()),
                    value: 2,
                },
            ],
        },
        "a 1\n(u8)b 2\n",
    );
}

#[test]
fn children_rest_beside_named_fields() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Doc {
        version: u32,
        #[serde(rename = "$children")]
        rest: BTreeMap<String, i32>,
    }
    let d: Doc = de("version 2; a 1; b 2");
    assert_eq!(d.version, 2);
    assert_eq!(d.rest, BTreeMap::from([("a".into(), 1), ("b".into(), 2)]));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Site {
    region: String,
    #[serde(default)]
    replicas: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Sites {
    site: BTreeMap<String, Site>,
}

#[test]
fn map_of_structs_keyed_by_argument() {
    round(
        &Sites {
            site: BTreeMap::from([
                (
                    "eu".into(),
                    Site {
                        region: "eu-west".into(),
                        replicas: 2,
                    },
                ),
                (
                    "us".into(),
                    Site {
                        region: "us-east".into(),
                        replicas: 3,
                    },
                ),
            ]),
        },
        "site \"eu\" {\n  region \"eu-west\"\n  replicas 2\n}\nsite \"us\" {\n  region \"us-east\"\n  replicas 3\n}\n",
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Env {
    env: BTreeMap<String, String>,
}

#[test]
fn map_of_scalars() {
    round(
        &Env {
            env: BTreeMap::from([("A".into(), "1".into()), ("B".into(), "2".into())]),
        },
        "env \"A\" \"1\"\nenv \"B\" \"2\"\n",
    );
}

#[test]
fn map_block_forms() {
    let e: Env = de("env { A \"1\"; B \"2\" }");
    assert_eq!(e.env["B"], "2");
    let e: Env = de("env A=\"1\" B=\"2\"");
    assert_eq!(e.env["A"], "1");
    let opts = Options::default().map_style(MapStyle::Block);
    let v = Env {
        env: BTreeMap::from([("A".into(), "1".into())]),
    };
    assert_eq!(
        kdl_serde::to_string_with(&v, &opts).unwrap(),
        "env {\n  A \"1\"\n}\n"
    );
}

#[test]
fn hash_map_prints_sorted() {
    let mut env = HashMap::new();
    for k in ["z", "a", "m", "b"] {
        env.insert(k.to_owned(), 1);
    }
    #[derive(Serialize)]
    struct Doc {
        env: HashMap<String, i32>,
    }
    assert_eq!(
        ser(&Doc { env }),
        "env \"a\" 1\nenv \"b\" 1\nenv \"m\" 1\nenv \"z\" 1\n"
    );
}

#[test]
fn map_with_integer_keys() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        port: BTreeMap<u16, String>,
    }
    round(
        &Doc {
            port: BTreeMap::from([(80, "http".into()), (443, "https".into())]),
        },
        "port 80 \"http\"\nport 443 \"https\"\n",
    );
    let d: Doc = de("port { \"80\" http }");
    assert_eq!(d.port[&80], "http");
}

#[test]
fn top_level_map() {
    let m: BTreeMap<String, u32> = de("a 1\nb 2");
    assert_eq!(m, BTreeMap::from([("a".into(), 1), ("b".into(), 2)]));
    assert_eq!(ser(&m), "a 1\nb 2\n");
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Fast,
    Safe,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Shape {
    Circle(f64),
    Rect { w: f64, h: f64 },
    Point,
    Line(f64, f64),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Drawing {
    mode: Mode,
    shape: Vec<Shape>,
}

#[test]
fn enums() {
    round(
        &Drawing {
            mode: Mode::Fast,
            shape: vec![
                Shape::Circle(1.5),
                Shape::Rect { w: 1.0, h: 2.0 },
                Shape::Point,
                Shape::Line(0.0, 3.0),
            ],
        },
        "mode \"fast\"\nshape (circle)1.5\n(rect)shape {\n  w 1.0\n  h 2.0\n}\nshape \"point\"\n(line)shape 0.0 3.0\n",
    );
}

#[test]
fn enum_spellings() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Doc {
        shape: Shape,
    }
    let read = |s| de::<Doc>(s).shape;
    assert_eq!(read("shape point"), Shape::Point);
    assert_eq!(read("shape { circle 2.0 }"), Shape::Circle(2.0));
    assert_eq!(
        read("shape { rect w=1.0 h=2.0 }"),
        Shape::Rect { w: 1.0, h: 2.0 }
    );
    assert_eq!(
        read("(rect)shape w=1.0 h=2.0"),
        Shape::Rect { w: 1.0, h: 2.0 }
    );
    assert_eq!(read("(circle)shape 3.0"), Shape::Circle(3.0));
}

#[test]
fn enum_variants_as_children() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        #[serde(rename = "$children")]
        shapes: Vec<Shape>,
    }
    round(
        &Doc {
            shapes: vec![
                Shape::Circle(1.0),
                Shape::Point,
                Shape::Rect { w: 1.0, h: 1.0 },
            ],
        },
        "circle 1.0\npoint\nrect {\n  w 1.0\n  h 1.0\n}\n",
    );
    let top: Vec<Shape> = de("circle 1.0\npoint");
    assert_eq!(top, [Shape::Circle(1.0), Shape::Point]);
}

#[test]
fn internally_tagged_enum() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "type", rename_all = "lowercase")]
    enum Store {
        File { path: String },
        Memory,
    }
    #[derive(Debug, PartialEq, Deserialize)]
    struct Doc {
        store: Store,
    }
    assert_eq!(
        de::<Doc>(r#"store type="file" path="/tmp""#).store,
        Store::File {
            path: "/tmp".into()
        }
    );
    assert_eq!(
        de::<Doc>("store { type file; path \"/x\" }").store,
        Store::File { path: "/x".into() }
    );
    assert_eq!(de::<Doc>("store type=memory").store, Store::Memory);
}

#[test]
fn untagged_enum() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(untagged)]
    enum Port {
        Number(u16),
        Named(String),
        Range { from: u16, to: u16 },
    }
    #[derive(Debug, PartialEq, Deserialize)]
    struct Doc {
        port: Vec<Port>,
    }
    let d: Doc = de("port 80; port http; port from=1 to=2");
    assert_eq!(
        d.port,
        [
            Port::Number(80),
            Port::Named("http".into()),
            Port::Range { from: 1, to: 2 }
        ]
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Scalars {
    a: i8,
    b: u64,
    c: i128,
    d: f32,
    e: f64,
    f: char,
    g: Option<u8>,
    h: (),
    i: (u8, String),
}

#[test]
fn scalars() {
    round(
        &Scalars {
            a: -5,
            b: u64::MAX,
            c: i128::MIN + 1,
            d: 0.5,
            e: 1e100,
            f: 'x',
            g: None,
            h: (),
            i: (1, "one".into()),
        },
        "a -5\nb 18446744073709551615\nc -170141183460469231731687303715884105727\nd 0.5\ne 1e100\nf \"x\"\nh\ni 1 \"one\"\n",
    );
}

#[test]
fn special_floats() {
    #[derive(Debug, Serialize, Deserialize)]
    struct F {
        a: f64,
        b: f64,
        c: f64,
    }
    let f: F = de("a #inf; b #-inf; c #nan");
    assert_eq!(f.a, f64::INFINITY);
    assert_eq!(f.b, f64::NEG_INFINITY);
    assert!(f.c.is_nan());
    assert_eq!(ser(&f), "a #inf\nb #-inf\nc #nan\n");
}

#[test]
fn null_is_none() {
    let s: Scalars = de("a 1; b 2; c 3; d 1.0; e 2; f x; g #null; h; i 1 x");
    assert_eq!(s.g, None);
    assert_eq!(s.e, 2.0);
}

#[test]
fn bytes_as_base64() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Blob {
        #[serde(with = "bytes")]
        data: Vec<u8>,
    }
    mod bytes {
        pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_bytes(v)
        }
        pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
            struct V;
            impl serde::de::Visitor<'_> for V {
                type Value = Vec<u8>;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("bytes")
                }
                fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                    Ok(v)
                }
            }
            d.deserialize_byte_buf(V)
        }
    }
    round(
        &Blob {
            data: b"hello".to_vec(),
        },
        "data (base64)\"aGVsbG8=\"\n",
    );
}

#[test]
fn annotated_values() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        size: Annotated<u8>,
        when: Annotated<String>,
        plain: Annotated<i32>,
        list: Vec<Annotated<i32>>,
        pos: Annotated<Site>,
    }
    round(
        &Doc {
            size: Annotated::new("u8", 5),
            when: Annotated::new("date", "2026-11-12".into()),
            plain: Annotated::plain(1),
            list: vec![Annotated::new("a", 1), Annotated::plain(2)],
            pos: Annotated::new(
                "site",
                Site {
                    region: "x".into(),
                    replicas: 0,
                },
            ),
        },
        "size (u8)5\nwhen (date)\"2026-11-12\"\nplain 1\nlist (a)1 2\n(site)pos {\n  region \"x\"\n  replicas 0\n}\n",
    );
}

#[test]
fn raw_passthrough() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Plugin {
        name: String,
        config: RawNode,
        settings: RawDocument,
    }
    let p: Plugin = de("name lint\nconfig 1 level=2 { x }\nsettings { a 1; b \"two\" }");
    assert_eq!(p.config.0.entries().len(), 2);
    assert_eq!(p.settings.0.nodes().len(), 2);
    assert_eq!(
        ser(&p),
        "name \"lint\"\nconfig 1 level=2 {\n  x\n}\nsettings {\n  a 1\n  b \"two\"\n}\n"
    );
    let whole: RawDocument = de("a 1\nb 2");
    assert_eq!(whole.0.nodes().len(), 2);
}

#[test]
fn nested_sequences() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Grid {
        row: Vec<Vec<i32>>,
    }
    round(
        &Grid {
            row: vec![vec![1, 2], vec![3]],
        },
        "row 1 2\nrow 3\n",
    );
    round(
        &Grid {
            row: vec![vec![1, 2]],
        },
        "row 1 2\n",
    );
    round(
        &Grid {
            row: vec![vec![], vec![1]],
        },
        "row {\n  -\n  - 1\n}\n",
    );
}

#[test]
fn optional_struct_elements() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        x: Vec<Option<i32>>,
    }
    round(
        &Doc {
            x: vec![Some(1), None],
        },
        "x 1 #null\n",
    );
}

#[test]
fn flatten() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Common {
        name: String,
    }
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        #[serde(flatten)]
        common: Common,
        extra: u32,
    }
    round(
        &Doc {
            common: Common { name: "n".into() },
            extra: 1,
        },
        "name \"n\"\nextra 1\n",
    );
}

#[test]
fn multiline_strings_stay_multiline() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        text: String,
        nested: Inner,
    }
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Inner {
        body: String,
    }
    let v = Doc {
        text: "line one\n  indented\n\nlast".into(),
        nested: Inner {
            body: "a \"\"\" b\\n\nend\n".into(),
        },
    };
    round(
        &v,
        "text \"\"\"\n  line one\n    indented\n\n  last\n  \"\"\"\nnested {\n  body \"\"\"\n    a \\\"\\\"\\\" b\\\\n\n    end\n\n    \"\"\"\n}\n",
    );
    let d: Doc = de("text \"\"\"\n    hi\n      there\n    \"\"\"\nnested { body \"x\" }");
    assert_eq!(d.text, "hi\n  there");
}

#[test]
fn compact_output() {
    let s = Sites {
        site: BTreeMap::from([(
            "eu".into(),
            Site {
                region: "r".into(),
                replicas: 1,
            },
        )]),
    };
    assert_eq!(
        kdl_serde::to_string(&s).unwrap(),
        "site \"eu\" { region \"r\"; replicas 1; }\n"
    );
}

#[test]
fn from_reader() {
    let s: Server = kdl_serde::from_reader("host h; port 1; debug #true".as_bytes()).unwrap();
    assert_eq!(s.port, 1);
}

#[test]
fn quoted_names() {
    let m: BTreeMap<String, i32> = BTreeMap::from([
        ("has space".into(), 1),
        ("1st".into(), 2),
        ("true".into(), 3),
    ]);
    let text = ser(&m);
    assert_eq!(text, "\"1st\" 2\n\"has space\" 1\n\"true\" 3\n");
    assert_eq!(de::<BTreeMap<String, i32>>(&text), m);
}

#[test]
fn alternative_spellings_from_the_readme() {
    let t: Team = de("member { - ada; - { $0 x } }");
    assert_eq!(t.member[0].name, "ada");
    let s: Sites = de("site { eu { region r } }");
    assert_eq!(s.site["eu"].region, "r");
    #[derive(Debug, PartialEq, Deserialize)]
    struct U {
        u: (),
    }
    assert_eq!(de::<U>("u #null"), U { u: () });
}

#[test]
fn dash_block_of_structs() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct P {
        x: i32,
    }
    #[derive(Debug, PartialEq, Deserialize)]
    struct Doc {
        p: Vec<P>,
    }
    let d: Doc = de("p { - { x 1 }; - x=2 }");
    assert_eq!(d.p, [P { x: 1 }, P { x: 2 }]);
}

#[test]
fn top_level_enum() {
    let s: Shape = de("rect w=1.0 h=2.0");
    assert_eq!(s, Shape::Rect { w: 1.0, h: 2.0 });
    assert_eq!(ser(&Shape::Circle(1.0)), "circle 1.0\n");
    assert_eq!(de::<Shape>("point"), Shape::Point);
}

#[test]
fn optional_struct_field() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        server: Option<Site>,
    }
    round(&Doc { server: None }, "");
    round(
        &Doc {
            server: Some(Site {
                region: "r".into(),
                replicas: 1,
            }),
        },
        "server {\n  region \"r\"\n  replicas 1\n}\n",
    );
}
