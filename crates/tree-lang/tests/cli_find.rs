use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn data_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(rel)
}

fn bin_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tree-lang"))
}

#[test]
fn find_supports_file_and_directory_inputs() {
    let rust_file = data_path("rust/rustc_parse_expr.rs");
    let rust_dir = data_path("rust");

    let out_file = bin_cmd()
        .args([
            "find",
            rust_file.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "-n",
            "^parse_expr_assoc_with$",
        ])
        .output()
        .expect("run tree-lang find (file)");
    assert!(out_file.status.success(), "stderr: {}", String::from_utf8_lossy(&out_file.stderr));
    let stdout_file = String::from_utf8_lossy(&out_file.stdout);
    assert!(stdout_file.contains("parse_expr_assoc_with"));

    let out_dir = bin_cmd()
        .args([
            "find",
            rust_dir.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "-n",
            "^parse_expr_assoc_with$",
        ])
        .output()
        .expect("run tree-lang find (dir)");
    assert!(out_dir.status.success(), "stderr: {}", String::from_utf8_lossy(&out_dir.stderr));
    let stdout_dir = String::from_utf8_lossy(&out_dir.stdout);
    assert!(stdout_dir.contains("parse_expr_assoc_with"));
}

#[test]
fn find_honors_exclude_regex() {
    let data_dir = data_path("");
    let out = bin_cmd()
        .args([
            "find",
            data_dir.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "-n",
            "^parse_expr_assoc_with$",
            "-e",
            "rustc_parse_expr\\.rs$",
        ])
        .output()
        .expect("run tree-lang find with exclude");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().is_empty(), "expected no matches, got:\n{stdout}");
}

#[test]
fn find_supports_parameter_filters() {
    let rust_file = data_path("rust/rustc_parse_expr.rs");
    let out = bin_cmd()
        .args([
            "find",
            rust_file.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "-n",
            "^parse_expr_assoc_with$",
            "-p",
            "^attrs$",
            "--param-name-at",
            "0:^self$",
            "--param-type-at",
            "1:^Bound$",
        ])
        .output()
        .expect("run tree-lang find with parameter filters");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("parse_expr_assoc_with"));
}

#[test]
fn find_supports_return_type_filter() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "--return-type",
        "^i32$",
        "--print-format",
        "{name}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn keep() -> i32 { 1 }\nfn skip() -> bool { false }\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("keep"), "stdout: {stdout}");
    assert!(!stdout.contains("skip"), "stdout: {stdout}");
}

#[test]
fn find_accepts_branch_kind_in_output_style_branch_if() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "branch(if)",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"fn f() { if true { 1 } else { 0 } }\n")
            .expect("write");
    }
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Branch(If)"),
        "expected Branch(If), got {stdout}"
    );
}

#[test]
fn find_accepts_loop_kind_in_output_style_loop_for() {
    let c_file = data_path("c/libgit2_repository.c");
    let out = bin_cmd()
        .args([
            "find",
            c_file.to_str().expect("utf8 path"),
            "-l",
            "c",
            "-k",
            "loop(for)",
        ])
        .output()
        .expect("run tree-lang find -k loop(for)");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Loop(For)"),
        "expected Loop(For) in output, got:\n{stdout}"
    );
}

#[test]
fn find_supports_pipeline_step() {
    let c_file = data_path("c/libgit2_repository.c");
    let out = bin_cmd()
        .args([
            "find",
            c_file.to_str().expect("utf8 path"),
            "-l",
            "c",
            "-k",
            "loop",
            "--step",
            "assign:ob:body",
            "--step",
            "has:ob:loop",
            "--step",
            "print:pipeline-ok",
        ])
        .output()
        .expect("run tree-lang find with --step pipeline");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("pipeline-ok"),
        "expected pipeline print, got:\n{stdout}"
    );
}

#[test]
fn find_supports_pipeline_is_step() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "c",
        "-k",
        "loop",
        "--step",
        "assign:x:node",
        "--step",
        "is:x:loop",
        "--step",
        "print:IS-OK",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"for(;;) {}")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("IS-OK"), "stdout: {stdout}");
}

#[test]
fn find_supports_pipeline_strip_step() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "--step",
        "assign:b:body",
        "--step",
        "strip",
        "--step",
        "print:STRIP-OK",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn f() { let _ = 1; for _ in 0..1 {} }\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("STRIP-OK"), "stdout: {stdout}");
}

#[test]
fn find_supports_pipeline_next_step() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "--step",
        "next:n",
        "--step",
        "print:NEXT-OK",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn a() {}\nfn b() {}\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().filter(|l| l.contains("NEXT-OK")).count(),
        1,
        "stdout: {stdout}"
    );
}

#[test]
fn find_supports_pipeline_expand_step() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "loop",
        "--step",
        "expand:inner",
        "--step",
        "print:EXPAND-OK",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"for _ in 0..1 { let _x = 0; }\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("EXPAND-OK"), "stdout: {stdout}");
}

#[test]
fn find_supports_print_and_print_format() {
    let rust_file = data_path("rust/rustc_parse_expr.rs");

    let out_print = bin_cmd()
        .args([
            "find",
            rust_file.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "-n",
            "^parse_expr_assoc_with$",
            "--print",
            "type,start,end",
        ])
        .output()
        .expect("run tree-lang find with --print fields");
    assert!(
        out_print.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_print.stderr)
    );
    let stdout_print = String::from_utf8_lossy(&out_print.stdout);
    assert!(stdout_print.lines().next().expect("one line").starts_with("FunctionDefinition\t"));

    let out_fmt = bin_cmd()
        .args([
            "find",
            rust_file.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "-n",
            "^parse_expr_assoc_with$",
            "--print-format",
            "{name}|{range}",
        ])
        .output()
        .expect("run tree-lang find with --print-format");
    assert!(
        out_fmt.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_fmt.stderr)
    );
    let stdout_fmt = String::from_utf8_lossy(&out_fmt.stdout);
    assert!(stdout_fmt.contains("parse_expr_assoc_with|"));
}

#[test]
fn find_print_format_supports_body_byte_placeholders() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "-n",
        "^f$",
        "--print-format",
        "{body_start_byte}|{body_end_byte}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn f() -> i32 {\n  1\n}\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().expect("one line");
    let mut parts = first.split('|');
    let b0: usize = parts.next().expect("a").parse().expect("start");
    let b1: usize = parts.next().expect("b").parse().expect("end");
    assert!(b1 > b0, "body span should be non-empty: {first}");
}

#[test]
fn find_print_format_supports_body_placeholder() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "-n",
        "^f$",
        "--print-format",
        "{name}|{body}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn f() -> i32 {\n  1\n}\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("f|{"), "stdout: {stdout}");
    assert!(stdout.contains('1'), "stdout: {stdout}");
}

#[test]
fn find_print_format_supports_language_placeholder() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "-n",
        "^g$",
        "--print-format",
        "{language}|{name}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin.write_all(b"fn g() {}\n").expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "rust|g", "stdout: {stdout}");
}

#[test]
fn find_print_format_supports_start_byte_placeholder() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "-n",
        "^h$",
        "--print-format",
        "{start_byte}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin.write_all(b"fn h() {}\n").expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim().parse::<usize>().is_ok(),
        "start_byte should be a number, got: {stdout}"
    );
}

#[test]
fn find_supports_stdin_input() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-l",
        "rust",
        "-k",
        "function_definition",
        "-n",
        "^parse_x$",
        "--print-format",
        "{file}|{name}",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn parse_x(a: i32) {}\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("<stdin>|parse_x"), "stdout: {stdout}");
}

#[test]
fn find_stdin_requires_explicit_language_when_default_is_auto() {
    let mut cmd = bin_cmd();
    cmd.args([
        "find",
        "-",
        "-k",
        "function_definition",
        "-n",
        "^parse_x$",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn tree-lang");
    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        stdin
            .write_all(b"fn parse_x(a: i32) {}\n")
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait output");
    assert!(!out.status.success(), "expected failure, stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--language auto cannot be used with stdin"),
        "stderr: {stderr}"
    );
}

#[test]
fn find_rejects_invalid_flag_combination() {
    let rust_file = data_path("rust/rustc_parse_expr.rs");
    let out = bin_cmd()
        .args([
            "find",
            rust_file.to_str().expect("utf8 path"),
            "-l",
            "rust",
            "-k",
            "function_definition",
            "--print",
            "--print-format",
            "{file}",
        ])
        .output()
        .expect("run invalid combination");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--print and --print-format cannot be used together"));
}

#[test]
fn find_auto_detects_supported_language_and_skips_unsupported() {
    let tmp_dir = std::env::temp_dir().join(format!(
        "tree_lang_auto_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&tmp_dir).expect("create temp dir");
    let rust_file = tmp_dir.join("ok.rs");
    let text_file = tmp_dir.join("skip.txt");
    fs::write(&rust_file, "fn auto_ok() -> i32 { 1 }\n").expect("write rust fixture");
    fs::write(&text_file, "not code\n").expect("write unsupported fixture");

    let out = bin_cmd()
        .args([
            "find",
            tmp_dir.to_str().expect("utf8 path"),
            "-l",
            "auto",
            "-k",
            "function_definition",
            "-n",
            "^auto_ok$",
            "--print-format",
            "{name}",
        ])
        .output()
        .expect("run tree-lang find with --language auto");

    fs::remove_dir_all(&tmp_dir).expect("cleanup temp dir");

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("auto_ok"), "stdout: {stdout}");
    assert!(
        stderr.contains("unsupported language for --language auto; skipping"),
        "stderr: {stderr}"
    );
}
