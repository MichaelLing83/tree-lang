use std::fs;
use std::path::PathBuf;

use tree_lang::{find_unified_kind, find_unified_kinds, Language, UnifiedKind};

fn read_data(path_under_data: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("data");
    path.push(path_under_data);
    fs::read_to_string(path).expect("read fixture")
}

#[test]
fn function_definition_search_works_on_real_projects() {
    let fixtures = [
        (Language::C, "c/libgit2_repository.c"),
        (Language::Cpp, "cpp/llvm_Instructions.cpp"),
        (Language::Rust, "rust/rustc_parse_expr.rs"),
        (Language::Python, "python/django_query.py"),
        (Language::Java, "java/spring_AnnotationUtils.java"),
    ];

    for (language, fixture) in fixtures {
        let source = read_data(fixture);
        let nodes =
            find_unified_kind(language, &source, UnifiedKind::FunctionDefinition).expect("parse");
        assert!(
            !nodes.is_empty(),
            "expected FunctionDefinition nodes in {:?} fixture {}",
            language,
            fixture
        );
    }
}

#[test]
fn multi_kind_search_returns_requested_kinds_only() {
    let source = read_data("python/django_query.py");
    let nodes = find_unified_kinds(
        Language::Python,
        &source,
        &[UnifiedKind::FunctionDefinition, UnifiedKind::If],
    )
    .expect("parse");

    assert!(!nodes.is_empty());
    assert!(nodes.iter().all(|n| {
        n.kind == UnifiedKind::FunctionDefinition || n.kind == UnifiedKind::If
    }));
}
