use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TRAV_TMP_ID: AtomicU64 = AtomicU64::new(0);

fn bin_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tree-lang"))
}

fn data_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(rel)
}

fn make_tmp_py() -> PathBuf {
    let n = TRAV_TMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "tree_lang_traverse_{}_{}.py",
        std::process::id(),
        n
    ));
    std::fs::write(
        &path,
        b"def foo():\n  if x:\n    for y in z:\n      pass\n",
    )
    .expect("write");
    path
}

#[test]
fn dfs_preorder_accepts_path_and_runs_with_step() {
    let path = make_tmp_py();
    let out = bin_cmd()
        .args([
            "dfs_preorder",
            path.to_str().expect("utf8"),
            "-l",
            "python",
            "--step",
            "emit:{type}",
        ])
        .output()
        .expect("run tree-lang dfs_preorder");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<_> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.len() >= 3,
        "expected multiple unified lines, got {lines:?}"
    );
}

#[test]
fn bfs_ltr_bfs_rtl_and_dfs_postorder_succeed() {
    let small_rust = data_path("rust/rustc_parse_expr.rs");
    for sub in ["bfs_ltr", "bfs_rtl", "dfs_postorder"] {
        let out = bin_cmd()
            .args([sub, small_rust.to_str().expect("path"), "-l", "rust"])
            .output()
            .expect(sub);
        assert!(out.status.success(), "{} stderr: {}", sub, String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn dfs_preorder_and_dfs_postorder_line_order_differs_for_py_fixture() {
    let path = make_tmp_py();
    let get_lines = |sub: &str| {
        let out = bin_cmd()
            .args([
                sub,
                path.to_str().expect("utf8"),
                "-l",
                "python",
                "--step",
                "emit:{type}",
            ])
            .output()
            .expect(sub);
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .filter(|l| l.contains("FunctionDefinition") || l.contains("If") || l.contains("For"))
            .collect::<Vec<_>>()
    };
    let pre = get_lines("dfs_preorder");
    let post = get_lines("dfs_postorder");
    let _ = std::fs::remove_file(&path);
    assert_eq!(pre.len(), post.len());
    assert_ne!(pre, post, "preorder and postorder should not match line order for nested if/for");
}

#[test]
fn traverse_stdin_requires_language_when_auto() {
    use std::process::Stdio;
    let mut child = bin_cmd()
        .args(["dfs_preorder", "-", "-l", "auto"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = writeln!(stdin, "def a():\n  pass");
    }
    let o = child.wait_with_output().expect("wait");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "expected error, got {err}");
    assert!(err.contains("auto") && err.contains("stdin"), "{err}");
}

#[test]
fn dfs_preorder_stdin_with_language_ok() {
    use std::process::Stdio;
    let mut child = bin_cmd()
        .args(["dfs_preorder", "-", "-l", "python"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut s = child.stdin.take().expect("stdin");
        let _ = writeln!(s, "def f():\n  if True: pass");
    }
    let o = child.wait_with_output().expect("wait");
    assert!(o.status.success(), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        !stdout.is_empty(),
        "expected at least one printed line for def + if"
    );
}
