//! Runs the `kdl-pretty` binary on temp files and stdin.

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

const MESSY: &str = "// hi\na   1;b {c}\n";
const TIDY: &str = "// hi\na 1\nb {\n  c\n}\n";

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kdl-pretty"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn stdin_to_stdout() {
    let out = run(&[], MESSY);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), TIDY);
    let out = run(&["-", "--indent", "4"], "a {b}");
    assert_eq!(text(&out.stdout), "a {\n    b\n}\n");
}

#[test]
fn stdin_check() {
    let out = run(&["--check"], MESSY);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"");
    assert_eq!(run(&["--check"], TIDY).status.code(), Some(0));
}

#[test]
fn files_in_place() {
    let dir = tempfile::tempdir().expect("tempdir");
    let messy = dir.path().join("messy.kdl");
    let tidy = dir.path().join("tidy.kdl");
    std::fs::write(&messy, MESSY).expect("write");
    std::fs::write(&tidy, TIDY).expect("write");
    let before = std::fs::metadata(&tidy)
        .and_then(|m| m.modified())
        .expect("mtime");
    std::thread::sleep(core::time::Duration::from_millis(20));

    let paths = [messy.to_str().expect("utf8"), tidy.to_str().expect("utf8")];
    let out = run(&["--check", paths[0], paths[1]], "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), format!("{}\n", paths[0]));
    assert_eq!(std::fs::read_to_string(&messy).expect("read"), MESSY);

    let out = run(&paths, "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(std::fs::read_to_string(&messy).expect("read"), TIDY);
    let after = std::fs::metadata(&tidy)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(before, after, "unchanged file was rewritten");

    assert_eq!(
        run(&["--check", paths[0], paths[1]], "").status.code(),
        Some(0)
    );
}

#[test]
fn parse_error_exits_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad = dir.path().join("bad.kdl");
    std::fs::write(&bad, "a {\n").expect("write");
    let out = run(&[bad.to_str().expect("utf8")], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("bad.kdl"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&bad).expect("read"), "a {\n");
    assert_eq!(run(&[], "a \"").status.code(), Some(2));
}

#[test]
fn bad_args_exit_2() {
    assert_eq!(run(&["--nope"], "").status.code(), Some(2));
    assert_eq!(run(&["--indent", "x"], "").status.code(), Some(2));
    assert_eq!(run(&["--help"], "").status.code(), Some(0));
}
