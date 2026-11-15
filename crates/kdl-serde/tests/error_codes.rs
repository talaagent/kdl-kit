//! Every error code in the workspace is documented in docs/errors.md, and
//! every documented code exists.

#![allow(clippy::unwrap_used)]

extern crate alloc;

use alloc::collections::BTreeSet;
use std::path::Path;

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `"kdl_<crate>::<code>"` string literals.
fn codes_in(text: &str) -> impl Iterator<Item = String> + '_ {
    text.match_indices("\"kdl_").filter_map(|(i, _)| {
        let rest = &text[i + 1..];
        let end = rest.find('"')?;
        let code = &rest[..end];
        let (krate, name) = code.split_once("::")?;
        let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_');
        (ok(krate) && ok(name)).then(|| code.to_owned())
    })
}

#[test]
fn codes_match_docs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let in_code: BTreeSet<String> = files
        .iter()
        .filter(|p| {
            !p.components()
                .any(|c| c.as_os_str() == "tests" || c.as_os_str() == "benches")
        })
        .flat_map(|p| codes_in(&std::fs::read_to_string(p).unwrap()).collect::<Vec<_>>())
        .collect();
    let docs = std::fs::read_to_string(root.join("docs/errors.md")).unwrap();
    let documented: BTreeSet<String> = docs
        .lines()
        .filter_map(|l| l.strip_prefix("## `")?.strip_suffix('`'))
        .map(str::to_owned)
        .collect();
    assert!(!in_code.is_empty());
    assert_eq!(in_code, documented, "docs/errors.md and the code disagree");
    for code in &documented {
        let anchor = format!("<a id=\"{}\"></a>", code.replace("::", "-"));
        assert!(docs.contains(&anchor), "missing anchor for {code}");
    }
}
