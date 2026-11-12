//! The official KDL v2 test suite (kdl-org/kdl `tests/test_cases`).
//!
//! Set `KDL_SPEC_DIR` to a kdl-org/kdl checkout (CI pins one); without it
//! the test passes with a note. Each `input/*.kdl` with an
//! `expected_kdl/*.kdl` must parse to the same meaning as the expected file
//! (parsed by the `kdl` crate as the oracle); every other input must fail.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;

use kdl_tree::Document;

/// Cases this parser gets wrong, with why. Keep empty.
const KNOWN_FAILURES: &[&str] = &[];

#[test]
fn official_test_suite() {
    let Some(dir) = std::env::var_os("KDL_SPEC_DIR") else {
        eprintln!("KDL_SPEC_DIR is not set; skipping the official KDL test suite");
        return;
    };
    let cases = PathBuf::from(dir).join("tests/test_cases");
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(cases.join("input"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "kdl"))
        .collect();
    inputs.sort();
    assert!(
        inputs.len() >= 300,
        "suite looks incomplete: {}",
        inputs.len()
    );

    let mut failures = Vec::new();
    for path in &inputs {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let input = String::from_utf8(std::fs::read(path).unwrap());
        let expected = std::fs::read_to_string(cases.join("expected_kdl").join(&name)).ok();
        let ours = input
            .as_deref()
            .map_err(|_| ())
            .and_then(|s| Document::parse(s).map_err(|_| ()));
        let problem = match (&ours, expected) {
            (Ok(doc), Some(expected)) => {
                let oracle = kdl::KdlDocument::parse_v2(&expected).unwrap();
                let want = common::normalized(&Document::from_kdl(&oracle));
                let got = common::normalized(doc);
                (got != want)
                    .then(|| format!("parsed differently:\n  got  {got:?}\n  want {want:?}"))
            }
            (Err(()), Some(_)) => Some("rejected a valid document".to_owned()),
            (Ok(_), None) => Some("accepted an invalid document".to_owned()),
            (Err(()), None) => None,
        };
        if let Some(p) = problem {
            failures.push((name, p));
        }
    }
    let unexpected: Vec<_> = failures
        .iter()
        .filter(|(n, _)| !KNOWN_FAILURES.contains(&n.as_str()))
        .collect();
    eprintln!(
        "official suite: {}/{} pass ({} known failures)",
        inputs.len() - failures.len(),
        inputs.len(),
        failures.len() - unexpected.len()
    );
    assert!(unexpected.is_empty(), "{unexpected:#?}");
}
