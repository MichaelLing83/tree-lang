use std::io::Write;
use std::process::{Command, Stdio};

fn bin_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tree-lang"))
}

#[test]
fn find_is_alias_for_dfs_preorder() {
    let find = run_with_stdin("find", "fn f() { if true { for _ in 0..1 { } } }\n");
    let dfs = run_with_stdin("dfs_preorder", "fn f() { if true { for _ in 0..1 { } } }\n");

    assert_eq!(find, dfs);
    assert!(find.contains("FunctionDefinition"), "stdout: {find}");
    assert!(find.contains("Branch(If)"), "stdout: {find}");
    assert!(find.contains("Loop(For)"), "stdout: {find}");
}

#[test]
fn find_alias_supports_step_pipeline() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-s",
        "node.is(loop)",
        "--print-format",
        "{type}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn f() { if true { } for _ in 0..1 { } }\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), vec!["Loop(For)"]);
}

#[test]
fn find_alias_rejects_removed_kind_flag() {
    let out = bin_cmd()
        .args(["find", "-", "-l", "rust", "-k", "loop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("run tree-lang find with removed -k");

    assert!(!out.status.success(), "expected removed -k to fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("found argument"),
        "stderr: {stderr}"
    );
}

#[test]
fn help_shows_find_as_dfs_preorder_alias() {
    let out = bin_cmd()
        .arg("--help")
        .output()
        .expect("run tree-lang --help");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("dfs-preorder") || stdout.contains("dfs_preorder"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("find"), "stdout: {stdout}");
}

fn run_with_stdin(subcommand: &str, source: &str) -> String {
    let mut cmd = bin_cmd();
    cmd.args([subcommand, "-", "-l", "rust", "--print-format", "{type}"]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin.write_all(source.as_bytes()).expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}
