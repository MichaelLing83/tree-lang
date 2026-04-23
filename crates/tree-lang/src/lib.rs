//! Parse source with tree-sitter and classify MVP syntax into [`tree_lang_core`] kinds.
//!
//! MVP: C, C++, Java, Rust, Python — function definitions, loops, `if`/`else`.

use std::collections::VecDeque;

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
    pub parameters: Vec<FunctionParameter>,
    pub return_type: Option<String>,
    pub body: Option<Span>,
}

/// One function parameter with normalized name and optional type text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionParameter {
    pub name: String,
    pub ty: Option<String>,
}

/// Order for visiting every node in a tree-sitter subtree (including anonymous nodes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TreeTraversal {
    /// Node first, then each child left-to-right (default depth-first walk).
    DfsPreorder,
    /// All children (left-to-right) first, then the node.
    DfsPostorder,
    /// Level order; within each level, left-to-right (children enqueued in parse order).
    BfsLtr,
    /// Level order; within each level, right-to-left (siblings: last child in parse order first).
    BfsRtl,
}

/// Visit `root` and every descendant in the given order; all children are visited in
/// tree-sitter `children` order, same as [`walk`].
pub fn for_each_subtree_node(
    root: Node<'_>,
    order: TreeTraversal,
    mut f: impl FnMut(Node<'_>),
) {
    match order {
        TreeTraversal::DfsPreorder => dfs_preorder(root, &mut f),
        TreeTraversal::DfsPostorder => dfs_postorder(root, &mut f),
        TreeTraversal::BfsLtr => bfs_subtree(root, &mut f, true),
        TreeTraversal::BfsRtl => bfs_subtree(root, &mut f, false),
    }
}

fn dfs_preorder(node: Node<'_>, f: &mut impl FnMut(Node<'_>)) {
    f(node);
    let mut c = node.walk();
    for child in node.children(&mut c) {
        dfs_preorder(child, f);
    }
}

fn dfs_postorder(node: Node<'_>, f: &mut impl FnMut(Node<'_>)) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        dfs_postorder(child, f);
    }
    f(node);
}

fn bfs_subtree(root: Node<'_>, f: &mut impl FnMut(Node<'_>), children_ltr: bool) {
    let mut q: VecDeque<Node<'_>> = VecDeque::new();
    q.push_back(root);
    while let Some(n) = q.pop_front() {
        f(n);
        let mut c = n.walk();
        let ch: Vec<Node<'_>> = n.children(&mut c).collect();
        if children_ltr {
            for child in ch {
                q.push_back(child);
            }
        } else {
            for child in ch.into_iter().rev() {
                q.push_back(child);
            }
        }
    }
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

/// Classify a single tree-sitter node as a [`MappedNode`], same rules as a depth-first find hit
/// (including `body` / consequence spans when present).
pub fn map_unified_node(language: Language, node: &Node<'_>) -> Option<MappedNode> {
    let span = Span::new(node.range().start_byte, node.range().end_byte);
    let mut m = classify(language, node.kind(), span)?;
    m.body = unified_body_span(language, *node, m.kind);
    Some(m)
}

/// If some unified node whose span is exactly `0..source.len()` is in `kinds`, return it: first
/// try the parse **root** (e.g. the whole text is one `for_statement`); else the single match
/// from the same walk `find_unified_kinds` would use (e.g. root is `translation_unit` with one
/// `for_statement` child). Used for `find` pipeline `is:VAR:KIND` (whole text is that construct,
/// not merely "contains" it).
pub fn match_root_as_unified(language: Language, source: &str, kinds: &[UnifiedKind]) -> Option<MappedNode> {
    let len = source.len();
    let tree = parse(language, source).ok()?;
    let root = tree.root_node();
    if let Some(m) = map_unified_node(language, &root) {
        if m.span.start_byte == 0 && m.span.end_byte == len && kinds.iter().any(|k| *k == m.kind) {
            return Some(m);
        }
    }
    find_unified_kinds(language, source, kinds)
        .ok()?
        .into_iter()
        .find(|m| m.span.start_byte == 0 && m.span.end_byte == len)
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
    if let Some(mut m) = classify(language, node.kind(), span) {
        m.body = unified_body_span(language, node, m.kind);
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
                let parameters = extract_function_parameters(language, node, source);
                let return_type = extract_function_return_type(language, node, source);
                let body = unified_body_span(language, node, UnifiedKind::FunctionDefinition);
                out.push(FunctionDefinitionNode {
                    span,
                    name,
                    parameters,
                    return_type,
                    body,
                });
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

fn span_from_field(node: Node<'_>, field: &str) -> Option<Span> {
    let n = node.child_by_field_name(field)?;
    Some(Span::new(n.range().start_byte, n.range().end_byte))
}

/// Primary body span for unified kinds: `body` for functions/loops, `consequence` for `if` when present.
fn unified_body_span(_language: Language, node: Node<'_>, kind: UnifiedKind) -> Option<Span> {
    match kind {
        UnifiedKind::If => span_from_field(node, "consequence").or_else(|| span_from_field(node, "body")),
        UnifiedKind::FunctionDefinition | UnifiedKind::Loop(_) => span_from_field(node, "body"),
    }
}

fn extract_function_parameters(
    language: Language,
    node: Node<'_>,
    source: &[u8],
) -> Vec<FunctionParameter> {
    let Some(parameters_node) = node.child_by_field_name("parameters") else {
        return match language {
            Language::C | Language::Cpp => node
                .child_by_field_name("declarator")
                .and_then(find_c_like_parameters_node)
                .map(|p| extract_c_like_parameters(p, source))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
    };
    match language {
        Language::C | Language::Cpp => extract_c_like_parameters(parameters_node, source),
        Language::Rust => extract_rust_parameters(parameters_node, source),
        Language::Python => extract_python_parameters(parameters_node, source),
        Language::Java => extract_java_parameters(parameters_node, source),
    }
}

fn extract_function_return_type(language: Language, node: Node<'_>, source: &[u8]) -> Option<String> {
    let normalize = |raw: String| {
        let trimmed = raw.trim();
        let without_arrow = trimmed.strip_prefix("->").map(str::trim).unwrap_or(trimmed);
        if without_arrow.is_empty() {
            None
        } else {
            Some(without_arrow.to_string())
        }
    };
    match language {
        Language::Rust | Language::Python => node
            .child_by_field_name("return_type")
            .and_then(|n| node_text(n, source))
            .and_then(normalize),
        Language::Java | Language::C | Language::Cpp => {
            node.child_by_field_name("type").and_then(|n| node_text(n, source))
        }
    }
}

fn find_c_like_parameters_node(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(parameters) = node.child_by_field_name("parameters") {
        return Some(parameters);
    }
    if let Some(inner) = node.child_by_field_name("declarator") {
        if let Some(parameters) = find_c_like_parameters_node(inner) {
            return Some(parameters);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(parameters) = find_c_like_parameters_node(child) {
            return Some(parameters);
        }
    }
    None
}

fn extract_c_like_parameters(parameters_node: Node<'_>, source: &[u8]) -> Vec<FunctionParameter> {
    let mut out = Vec::new();
    let mut cursor = parameters_node.walk();
    for child in parameters_node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let declarator = child.child_by_field_name("declarator");
        let declarator_name_node =
            declarator.and_then(c_like_declarator_name_node).or_else(|| child.child_by_field_name("name"));
        let name = declarator_name_node
            .and_then(|n| node_text(n, source))
            .or_else(|| declarator.and_then(|d| extract_c_like_declarator_name(d, source)));
        let mut ty = child.child_by_field_name("type").and_then(|t| node_text(t, source));
        if let Some(name_node) = declarator_name_node {
            ty = c_like_parameter_type_from_full(child, source, name_node).or_else(|| {
                let name_ref = name.as_deref().unwrap_or_default();
                if let (Some(base_type), Some(decl)) = (&ty, declarator) {
                    let suffix = c_like_declarator_suffix(decl, source, name_ref);
                    if suffix.is_empty() {
                        Some(base_type.clone())
                    } else {
                        Some(format!("{} {}", base_type, suffix))
                    }
                } else {
                    ty.clone()
                }
            });
        } else if let Some(base_type) = &ty {
            if base_type.is_empty() {
                ty = None;
            }
        }
        if let Some(name) = name {
            out.push(FunctionParameter { name, ty });
        }
    }
    out
}

fn extract_rust_parameters(parameters_node: Node<'_>, source: &[u8]) -> Vec<FunctionParameter> {
    let mut out = Vec::new();
    let mut cursor = parameters_node.walk();
    for child in parameters_node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        if child.kind() == "self_parameter" {
            let ty = child.child_by_field_name("type").and_then(|t| node_text(t, source));
            out.push(FunctionParameter {
                name: "self".to_string(),
                ty,
            });
            continue;
        }
        let name = child
            .child_by_field_name("pattern")
            .and_then(|p| first_identifier_descendant(p, source))
            .or_else(|| child.child_by_field_name("name").and_then(|n| node_text(n, source)));
        let ty = child.child_by_field_name("type").and_then(|t| node_text(t, source));
        if let Some(name) = name {
            out.push(FunctionParameter { name, ty });
        }
    }
    out
}

fn extract_python_parameters(parameters_node: Node<'_>, source: &[u8]) -> Vec<FunctionParameter> {
    let mut out = Vec::new();
    let mut cursor = parameters_node.walk();
    for child in parameters_node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let name = child
            .child_by_field_name("name")
            .and_then(|n| node_text(n, source))
            .or_else(|| first_identifier_descendant(child, source));
        let ty = child.child_by_field_name("type").and_then(|t| node_text(t, source));
        if let Some(name) = name {
            out.push(FunctionParameter { name, ty });
        }
    }
    out
}

fn extract_java_parameters(parameters_node: Node<'_>, source: &[u8]) -> Vec<FunctionParameter> {
    let mut out = Vec::new();
    let mut cursor = parameters_node.walk();
    for child in parameters_node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let name_node = child.child_by_field_name("name");
        let name = name_node
            .and_then(|n| node_text(n, source))
            .or_else(|| first_identifier_descendant(child, source));
        let mut ty = child.child_by_field_name("type").and_then(|t| node_text(t, source));
        if let Some(name_node) = name_node {
            if let Some(full_ty) = java_parameter_type_from_full(child, source, name_node) {
                ty = Some(full_ty);
            }
        }
        if let Some(name) = name {
            out.push(FunctionParameter { name, ty });
        }
    }
    out
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

fn first_identifier_descendant(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => return node_text(node, source),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = first_identifier_descendant(child, source) {
            return Some(name);
        }
    }
    None
}

fn c_like_declarator_suffix(declarator: Node<'_>, source: &[u8], name: &str) -> String {
    let raw = declarator.utf8_text(source).ok().unwrap_or_default();
    raw.replacen(name, "", 1).trim().to_string()
}

fn c_like_parameter_type_from_full(
    param_node: Node<'_>,
    source: &[u8],
    name_node: Node<'_>,
) -> Option<String> {
    let p_start = param_node.start_byte();
    let p_end = param_node.end_byte();
    let n_start = name_node.start_byte();
    let n_end = name_node.end_byte();
    if n_start < p_start || n_end > p_end || n_start >= n_end {
        return None;
    }
    let param_bytes = &source[p_start..p_end];
    let rel_start = n_start - p_start;
    let rel_end = n_end - p_start;
    let mut bytes = Vec::with_capacity(param_bytes.len().saturating_sub(rel_end - rel_start));
    bytes.extend_from_slice(&param_bytes[..rel_start]);
    bytes.extend_from_slice(&param_bytes[rel_end..]);
    let ty = std::str::from_utf8(&bytes).ok()?.trim().to_string();
    if ty.is_empty() {
        None
    } else {
        Some(ty)
    }
}

fn java_parameter_type_from_full(
    param_node: Node<'_>,
    source: &[u8],
    name_node: Node<'_>,
) -> Option<String> {
    let p_start = param_node.start_byte();
    let p_end = param_node.end_byte();
    let n_start = name_node.start_byte();
    let n_end = name_node.end_byte();
    if n_start < p_start || n_end > p_end || n_start >= n_end {
        return None;
    }
    let param_bytes = &source[p_start..p_end];
    let rel_start = n_start - p_start;
    let rel_end = n_end - p_start;
    let mut bytes = Vec::with_capacity(param_bytes.len().saturating_sub(rel_end - rel_start));
    bytes.extend_from_slice(&param_bytes[..rel_start]);
    bytes.extend_from_slice(&param_bytes[rel_end..]);
    let ty = std::str::from_utf8(&bytes).ok()?.trim().to_string();
    if ty.is_empty() {
        None
    } else {
        Some(ty)
    }
}

fn c_like_declarator_name_node(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "identifier" | "field_identifier" | "qualified_identifier" | "operator_name"
        | "destructor_name" => return Some(node),
        _ => {}
    }
    if let Some(inner) = node.child_by_field_name("declarator") {
        if let Some(name_node) = c_like_declarator_name_node(inner) {
            return Some(name_node);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name_node) = c_like_declarator_name_node(child) {
            return Some(name_node);
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
