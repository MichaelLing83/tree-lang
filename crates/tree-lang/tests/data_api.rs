use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use tree_lang::{
    find_function_definitions, find_unified_kind, find_unified_kinds, Language, UnifiedKind,
};

fn read_data(path_under_data: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("data");
    path.push(path_under_data);
    fs::read_to_string(path).expect("read fixture")
}

fn data_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("data");
    path
}

#[derive(Debug, Deserialize)]
struct ExpectedFile {
    fixture: String,
    count: usize,
    names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExpectedFunctionDefinition {
    c: ExpectedFile,
    cpp: ExpectedFile,
    rust: ExpectedFile,
    python: ExpectedFile,
    java: ExpectedFile,
}

#[derive(Debug, Deserialize)]
struct Expectations {
    function_definition: ExpectedFunctionDefinition,
}

fn read_expectations() -> Expectations {
    let mut path = data_root();
    path.push("EXPECTATIONS.toml");
    let raw = fs::read_to_string(path).expect("read expectations");
    toml::from_str(&raw).expect("parse expectations")
}

#[test]
fn function_definition_search_works_on_real_projects() {
    let expected = read_expectations().function_definition;
    let fixtures = [
        (Language::C, expected.c),
        (Language::Cpp, expected.cpp),
        (Language::Rust, expected.rust),
        (Language::Python, expected.python),
        (Language::Java, expected.java),
    ];

    for (language, expected_file) in fixtures {
        let source = read_data(&expected_file.fixture);
        let nodes =
            find_unified_kind(language, &source, UnifiedKind::FunctionDefinition).expect("parse");
        let functions = find_function_definitions(language, &source).expect("parse");
        assert_eq!(
            functions.len(),
            nodes.len(),
            "count mismatch between kind-filter and function extraction in {:?} fixture {}",
            language,
            expected_file.fixture
        );
        assert!(
            functions.iter().all(|f| !f.name.trim().is_empty()),
            "expected every FunctionDefinition name to be non-empty in {:?} fixture {}",
            language,
            expected_file.fixture
        );
        assert_eq!(
            functions.len(),
            expected_file.count,
            "unexpected FunctionDefinition count in {:?} fixture {}",
            language,
            expected_file.fixture
        );
        for required_name in &expected_file.names {
            assert!(
                functions.iter().any(|f| &f.name == required_name),
                "required function name '{}' not found in {:?} fixture {}",
                required_name,
                language,
                expected_file.fixture
            );
        }
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
