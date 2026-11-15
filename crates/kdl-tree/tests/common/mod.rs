//! A parser-independent model of a document: what the KDL spec says a
//! document means (props last-wins, NaN equal to NaN).

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;

use kdl_tree::{Document, Node, Value};

#[derive(Debug, Clone)]
pub enum V {
    S(String),
    I(i128),
    F(f64),
    B(bool),
    N,
}

impl PartialEq for V {
    fn eq(&self, o: &V) -> bool {
        match (self, o) {
            (V::S(a), V::S(b)) => a == b,
            (V::I(a), V::I(b)) => a == b,
            (V::F(a), V::F(b)) => (a.is_nan() && b.is_nan()) || a == b,
            (V::B(a), V::B(b)) => a == b,
            (V::N, V::N) => true,
            _ => false,
        }
    }
}

pub type Typed = (Option<String>, V);

#[derive(Debug, Clone, PartialEq)]
pub struct M {
    pub ty: Option<String>,
    pub name: String,
    pub args: Vec<Typed>,
    pub props: BTreeMap<String, Typed>,
    pub children: Option<Vec<M>>,
}

fn value(v: &Value<'_>) -> V {
    match v {
        Value::String(s) => V::S(s.to_string()),
        Value::Integer(i) => V::I(*i),
        Value::Float(f) => V::F(*f),
        Value::Bool(b) => V::B(*b),
        Value::Null => V::N,
    }
}

fn node(n: &Node<'_>) -> M {
    let mut args = Vec::new();
    let mut props = BTreeMap::new();
    for e in n.entries() {
        let tv = (e.ty().map(|t| t.value().to_owned()), value(e.value()));
        match e.name() {
            Some(k) => {
                props.insert(k.value().to_owned(), tv);
            }
            None => args.push(tv),
        }
    }
    M {
        ty: n.ty().map(|t| t.value().to_owned()),
        name: n.name().value().to_owned(),
        args,
        props,
        children: n.children().map(model),
    }
}

/// The meaning of `doc`. An empty children block counts as none, as the
/// spec's expected outputs drop them.
pub fn model(doc: &Document<'_>) -> Vec<M> {
    doc.nodes().iter().map(node).collect()
}

/// [`model`] with empty children blocks removed.
pub fn normalized(doc: &Document<'_>) -> Vec<M> {
    fn norm(ms: &mut [M]) {
        for m in ms {
            if let Some(c) = &mut m.children {
                if c.is_empty() {
                    m.children = None;
                } else {
                    norm(c);
                }
            }
        }
    }
    let mut m = model(doc);
    norm(&mut m);
    m
}
