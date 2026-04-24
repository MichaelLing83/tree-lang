use tree_lang::{extract_unified, parse, BranchKind, Language, LoopKind, UnifiedKind};

fn kinds_sample(lang: Language, source: &str) -> Vec<UnifiedKind> {
    let tree = parse(lang, source).expect("parse");
    extract_unified(lang, &tree)
        .into_iter()
        .map(|n| n.kind)
        .collect()
}

#[test]
fn python_function_loop_if() {
    let src = r#"
def f():
    for i in range(3):
        pass
    while True:
        break
    if True:
        pass
    else:
        pass
"#;
    let k = kinds_sample(Language::Python, src);
    assert_eq!(
        k,
        vec![
            UnifiedKind::FunctionDefinition,
            UnifiedKind::Loop(LoopKind::For),
            UnifiedKind::Loop(LoopKind::While),
            UnifiedKind::Branch(BranchKind::If),
        ]
    );
}

#[test]
fn rust_function_loop_if() {
    let src = r#"
fn main() {
    for x in 0..1 {}
    while false {}
    loop {}
    if true { } else { }
}
"#;
    let k = kinds_sample(Language::Rust, src);
    assert_eq!(
        k,
        vec![
            UnifiedKind::FunctionDefinition,
            UnifiedKind::Loop(LoopKind::For),
            UnifiedKind::Loop(LoopKind::While),
            UnifiedKind::Loop(LoopKind::Infinite),
            UnifiedKind::Branch(BranchKind::If),
        ]
    );
}

#[test]
fn java_method_loops_if() {
    let src = r#"
class T {
  void m() {
    for (int i = 0; i < 10; i++) {}
    for (String s : arr) {}
    while (true) {}
    do { } while (false);
    if (true) { } else { }
  }
}
"#;
    let k = kinds_sample(Language::Java, src);
    assert_eq!(
        k,
        vec![
            UnifiedKind::FunctionDefinition,
            UnifiedKind::Loop(LoopKind::For),
            UnifiedKind::Loop(LoopKind::ForEach),
            UnifiedKind::Loop(LoopKind::While),
            UnifiedKind::Loop(LoopKind::DoWhile),
            UnifiedKind::Branch(BranchKind::If),
        ]
    );
}

#[test]
fn c_function_loops_if() {
    let src = r#"
void f() {
  for (;;) {}
  while (0) {}
  do { } while(0);
  if (1) { } else { }
}
"#;
    let k = kinds_sample(Language::C, src);
    assert_eq!(
        k,
        vec![
            UnifiedKind::FunctionDefinition,
            UnifiedKind::Loop(LoopKind::For),
            UnifiedKind::Loop(LoopKind::While),
            UnifiedKind::Loop(LoopKind::DoWhile),
            UnifiedKind::Branch(BranchKind::If),
        ]
    );
}

#[test]
fn rust_match_classified_as_branch_match() {
    let src = r#"
fn demo(x: i32) {
    match x {
        0 => {}
        _ => {}
    }
}
"#;
    let k = kinds_sample(Language::Rust, src);
    assert!(
        k.contains(&UnifiedKind::Branch(BranchKind::Match)),
        "expected Branch(Match), got {k:?}"
    );
}

#[test]
fn c_switch_classified_as_branch_switch() {
    let src = r#"
void f(int x) {
  switch (x) { case 0: break; }
}
"#;
    let k = kinds_sample(Language::C, src);
    assert!(
        k.contains(&UnifiedKind::Branch(BranchKind::Switch)),
        "expected Branch(Switch), got {k:?}"
    );
}

#[test]
fn cpp_function_loops_if() {
    let src = r#"
void f() {
  for (int i = 0; i < 10; i++) {}
  for (auto x : v) {}
  while (true) {}
  do { } while(false);
  if (true) { } else { }
}
"#;
    let k = kinds_sample(Language::Cpp, src);
    assert_eq!(
        k,
        vec![
            UnifiedKind::FunctionDefinition,
            UnifiedKind::Loop(LoopKind::For),
            UnifiedKind::Loop(LoopKind::ForEach),
            UnifiedKind::Loop(LoopKind::While),
            UnifiedKind::Loop(LoopKind::DoWhile),
            UnifiedKind::Branch(BranchKind::If),
        ]
    );
}
