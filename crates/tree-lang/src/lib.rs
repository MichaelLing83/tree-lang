//! Parse source with tree-sitter and classify MVP syntax into [`tree_lang_core`] kinds.
//!
//! MVP: C, C++, Java, Rust, Python — function definitions, loops, `if`/`else`.

mod classify;
mod language;

pub use language::Language;
pub use tree_lang_core::{LoopKind, MappedNode, Span, UnifiedKind};
pub use tree_sitter::{Node, Tree};

use classify::classify;

/// Parse `source` as the given [`Language`].
pub fn parse(language: Language, source: &str) -> Result<Tree, ParseError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.tree_sitter_language())
        .map_err(|_| ParseError::Language)?;
    let tree = parser.parse(source, None).ok_or(ParseError::Cancelled)?;
    Ok(tree)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Failed to assign the grammar to the parser.
    Language,
    /// Parsing was cancelled (tree-sitter returned no tree).
    Cancelled,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Language => f.write_str("invalid tree-sitter language"),
            ParseError::Cancelled => f.write_str("parse cancelled"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A function definition node with its normalized name (when available).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionDefinitionNode {
    pub span: Span,
    pub name: String,
}

/// Depth-first walk of `tree`, returning every node that maps to a [`UnifiedKind`].
pub fn extract_unified(language: Language, tree: &Tree) -> Vec<MappedNode> {
    let mut out = Vec::new();
    walk(language, tree.root_node(), &mut out, None);
    out
}

/// Depth-first walk of `tree`, returning nodes whose kind is in `kinds`.
pub fn extract_unified_kinds(language: Language, tree: &Tree, kinds: &[UnifiedKind]) -> Vec<MappedNode> {
    let mut out = Vec::new();
    walk(language, tree.root_node(), &mut out, Some(kinds));
    out
}

/// Parse source and return nodes matching a single target [`UnifiedKind`].
pub fn find_unified_kind(
    language: Language,
    source: &str,
    kind: UnifiedKind,
) -> Result<Vec<MappedNode>, ParseError> {
    let tree = parse(language, source)?;
    Ok(extract_unified_kinds(language, &tree, &[kind]))
}

/// Parse source and return nodes matching any of `kinds`.
pub fn find_unified_kinds(
    language: Language,
    source: &str,
    kinds: &[UnifiedKind],
) -> Result<Vec<MappedNode>, ParseError> {
    let tree = parse(language, source)?;
    Ok(extract_unified_kinds(language, &tree, kinds))
}

/// Parse source and return function-definition nodes with extracted names.
pub fn find_function_definitions(
    language: Language,
    source: &str,
) -> Result<Vec<FunctionDefinitionNode>, ParseError> {
    let tree = parse(language, source)?;
    let mut out = Vec::new();
    walk_function_definitions(language, source.as_bytes(), tree.root_node(), &mut out);
    Ok(out)
}

fn walk(language: Language, node: Node<'_>, out: &mut Vec<MappedNode>, kinds: Option<&[UnifiedKind]>) {
    let span = Span::new(node.range().start_byte, node.range().end_byte);
    if let Some(m) = classify(language, node.kind(), span) {
        let matched = match kinds {
            Some(filters) => filters.contains(&m.kind),
            None => true,
        };
        if matched {
            out.push(m);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(language, child, out, kinds);
    }
}

fn walk_function_definitions(
    language: Language,
    source: &[u8],
    node: Node<'_>,
    out: &mut Vec<FunctionDefinitionNode>,
) {
    let span = Span::new(node.range().start_byte, node.range().end_byte);
    if let Some(m) = classify(language, node.kind(), span) {
        if m.kind == UnifiedKind::FunctionDefinition {
            if let Some(name) = extract_function_name(language, node, source) {
                out.push(FunctionDefinitionNode { span, name });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_function_definitions(language, source, child, out);
    }
}

fn extract_function_name(language: Language, node: Node<'_>, source: &[u8]) -> Option<String> {
    match language {
        Language::Python | Language::Java | Language::Rust => {
            let name_node = node.child_by_field_name("name")?;
            node_text(name_node, source)
        }
        Language::C | Language::Cpp => {
            let declarator = node.child_by_field_name("declarator")?;
            extract_c_like_declarator_name(declarator, source)
        }
    }
}

fn extract_c_like_declarator_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "qualified_identifier" | "operator_name"
        | "destructor_name" => return node_text(node, source),
        _ => {}
    }

    if let Some(inner) = node.child_by_field_name("declarator") {
        if let Some(name) = extract_c_like_declarator_name(inner, source) {
            return Some(name);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = extract_c_like_declarator_name(child, source) {
            return Some(name);
        }
    }
    None
}

fn node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    let text = node.utf8_text(source).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
