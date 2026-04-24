//! Ordered find/traverse pipeline: ``--step`` with `assign|a`, `next:`, `expand:`, `has|h`, `is|i`,
//! `print|p`, `strip`. See `find --help` and `tree-lang dfs_preorder` / `dfs_postorder` / `bfs_ltr` / `bfs_rtl` --help`.

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::Node;
use tree_lang::{
    find_unified_kinds, format_kind, kinds_from_cli, map_unified_node, match_root_as_unified, parse,
    BranchKind, Language, MappedNode, Span, UnifiedKind,
};

use crate::{byte_to_line_col, render_template, span_text};

#[derive(Debug, Clone)]
pub enum PipelineStep {
    Assign { name: String, source: String },
    /// Next named sibling in the full parse tree that classifies as a unified node; bind span to
    /// `var` and set `current` to that node.
    Next { var: String },
    /// `current` must have a `body` span: bind that span to `var` and move `current` into the
    /// body (re-parse; if the body root is unified and spans the whole body, use that node).
    Expand { var: String },
    Has { var: String, kind_cli: String },
    /// Root of the parse of `var`'s span must be a unified node spanning the whole slice and match KIND.
    Is { var: String, kind_cli: String },
    /// Narrow `current` to the leftmost loop / `if` / function in its source span (drop leading and trailing non-block syntax).
    Strip,
    Print { template: String },
}

pub struct PipelineRunOutcome {
    /// When true, emit one line for the **root** match with `--print` / default output.
    pub need_default_print: bool,
}

/// Longer prefixes first so `is:` wins over `i:` and `assign:` over `a:`.
fn split_step_prefix(s: &str) -> Option<(&'static str, &str)> {
    const PAIRS: &[(&str, &'static str)] = &[
        ("assign:", "assign"),
        ("expand:", "expand"),
        ("next:", "next"),
        ("strip:", "strip"),
        ("print:", "print"),
        ("has:", "has"),
        ("is:", "is"),
        ("a:", "assign"),
        ("h:", "has"),
        ("p:", "print"),
        ("i:", "is"),
        ("s:", "strip"),
    ];
    for (prefix, kind) in PAIRS {
        if let Some(rest) = s.strip_prefix(prefix) {
            return Some((*kind, rest));
        }
    }
    None
}

pub fn parse_steps(steps: &[String]) -> Result<Vec<PipelineStep>, String> {
    let mut out = Vec::new();
    for s in steps {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        if s == "strip" {
            out.push(PipelineStep::Strip);
            continue;
        }
        let Some((step_kind, rest)) = split_step_prefix(s) else {
            return Err(format!(
                "unknown --step {s:?}; see find --help (assign, next:, expand:, has, is, print, strip, ...)"
            ));
        };
        match step_kind {
            "assign" => {
                let (name, source) = rest.split_once(':').ok_or_else(|| {
                    format!("assign expects assign:NAME:SOURCE (or a:...), got {s:?}")
                })?;
                if name.is_empty() || source.is_empty() {
                    return Err(format!("invalid assign step: {s:?}"));
                }
                out.push(PipelineStep::Assign {
                    name: name.to_string(),
                    source: source.to_string(),
                });
            }
            "next" => {
                let var = rest.trim();
                if var.is_empty() {
                    return Err(format!("next expects next:NAME, got {s:?}"));
                }
                if var.contains(':') {
                    return Err(format!("next: variable name must not contain ':', got {s:?}"));
                }
                out.push(PipelineStep::Next {
                    var: var.to_string(),
                });
            }
            "expand" => {
                let var = rest.trim();
                if var.is_empty() {
                    return Err(format!("expand expects expand:NAME, got {s:?}"));
                }
                if var.contains(':') {
                    return Err(format!("expand: variable name must not contain ':', got {s:?}"));
                }
                out.push(PipelineStep::Expand {
                    var: var.to_string(),
                });
            }
            "has" | "is" => {
                let (var, kind_cli) = rest.split_once(':').ok_or_else(|| {
                    format!("{step_kind} expects {step_kind}:VAR:KIND (e.g. has:ob:loop:for), got {s:?}")
                })?;
                if var.is_empty() || kind_cli.is_empty() {
                    return Err(format!("invalid {step_kind} step: {s:?}"));
                }
                if step_kind == "has" {
                    out.push(PipelineStep::Has {
                        var: var.to_string(),
                        kind_cli: kind_cli.to_string(),
                    });
                } else {
                    out.push(PipelineStep::Is {
                        var: var.to_string(),
                        kind_cli: kind_cli.to_string(),
                    });
                }
            }
            "strip" => {
                if !rest.is_empty() {
                    return Err(format!(
                        "strip must be the word `strip`, or `strip:` / `s:` with nothing after, got {s:?}"
                    ));
                }
                out.push(PipelineStep::Strip);
            }
            "print" => {
                out.push(PipelineStep::Print {
                    template: rest.to_string(),
                });
            }
            _ => unreachable!(),
        }
    }
    Ok(out)
}

/// [`UnifiedKind`]s that count as "block-shaped" for `strip`: function, `if`, and all loop forms.
fn block_shape_kinds() -> Result<Vec<UnifiedKind>, String> {
    let mut v = kinds_from_cli("function_definition")?;
    v.extend(kinds_from_cli("branch")?);
    v.extend(kinds_from_cli("loop")?);
    Ok(v)
}

fn region_for_source(m: &MappedNode, from: &str) -> Result<Span, String> {
    let s = from.trim();
    match s {
        "node" | "node_span" | "content" => Ok(m.span),
        "body" => m
            .body
            .ok_or_else(|| "this node has no body field".to_string()),
        "consequence" => {
            if m.kind != UnifiedKind::Branch(BranchKind::If) {
                return Err(
                    "--assign :consequence is only valid when the current node is branch:if"
                        .to_string(),
                );
            }
            m.body
                .ok_or_else(|| "if node has no consequence/body span".to_string())
        }
        _ => Err(format!(
            "unknown assign source {from:?}; use node, node_span, content, body, or consequence"
        )),
    }
}

fn offset_mapped_node(m: &MappedNode, off: usize) -> MappedNode {
    let sh = |sp: Span| Span {
        start_byte: sp.start_byte + off,
        end_byte: sp.end_byte + off,
    };
    MappedNode {
        kind: m.kind,
        span: sh(m.span),
        body: m.body.map(sh),
    }
}

/// First tree-sitter subtree in preorder whose byte range exactly matches `start..end`.
fn ts_node_exact_range<'a>(node: Node<'a>, start: usize, end: usize) -> Option<Node<'a>> {
    let r = node.range();
    if r.start_byte == start && r.end_byte == end {
        return Some(node);
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if let Some(n) = ts_node_exact_range(child, start, end) {
            return Some(n);
        }
    }
    None
}

/// Returns `Ok(None)` if this root match does not pass the pipeline (filter out).
/// Returns `Err` on invalid --step usage for the current node.
pub fn run_unified_pipeline(
    path: &Path,
    source: &str,
    language: Language,
    root: &MappedNode,
    steps: &[PipelineStep],
) -> Result<Option<PipelineRunOutcome>, String> {
    let mut current = *root;
    let mut bindings: HashMap<String, Span> = HashMap::new();
    let mut has_print = false;

    for step in steps {
        match step {
            PipelineStep::Assign { name, source: src } => {
                let sp = match region_for_source(&current, src) {
                    Ok(sp) if sp.start_byte < sp.end_byte => sp,
                    Ok(_) => return Ok(None),
                    Err(e) => return Err(e),
                };
                bindings.insert(name.clone(), sp);
            }
            PipelineStep::Next { var } => {
                let tree = parse(language, source)
                    .map_err(|e: tree_lang::ParseError| format!("next: parse file: {e}"))?;
                let root = tree.root_node();
                let sp = current.span;
                let Some(ts) = ts_node_exact_range(root, sp.start_byte, sp.end_byte) else {
                    return Err("next: current span not found in tree-sitter tree".to_string());
                };
                let mut sib = ts.next_named_sibling();
                let found = 'm: {
                    while let Some(n) = sib {
                        if let Some(m) = map_unified_node(language, &n) {
                            break 'm Some(m);
                        }
                        sib = n.next_named_sibling();
                    }
                    None
                };
                let Some(m) = found else {
                    return Ok(None);
                };
                bindings.insert(var.clone(), m.span);
                current = m;
            }
            PipelineStep::Expand { var } => {
                let b = current
                    .body
                    .ok_or_else(|| "expand: current unified node has no `body` field".to_string())?;
                if b.start_byte >= b.end_byte {
                    return Ok(None);
                }
                bindings.insert(var.clone(), b);
                let slice = source
                    .get(b.start_byte..b.end_byte)
                    .ok_or_else(|| "expand: body out of range".to_string())?;
                let body_tree = parse(language, slice)
                    .map_err(|e: tree_lang::ParseError| format!("expand: parse body: {e}"))?;
                let br = body_tree.root_node();
                if let Some(m) = map_unified_node(language, &br) {
                    if m.span.start_byte == 0 && m.span.end_byte == slice.len() {
                        current = offset_mapped_node(&m, b.start_byte);
                    } else {
                        current = MappedNode {
                            kind: current.kind,
                            span: b,
                            body: None,
                        };
                    }
                } else {
                    current = MappedNode {
                        kind: current.kind,
                        span: b,
                        body: None,
                    };
                }
            }
            PipelineStep::Has { var, kind_cli } => {
                let sp = *bindings
                    .get(var)
                    .ok_or_else(|| format!("--step has: unknown variable {var:?}"))?;
                if sp.start_byte >= sp.end_byte {
                    return Ok(None);
                }
                let slice = source
                    .get(sp.start_byte..sp.end_byte)
                    .ok_or_else(|| "body span out of range".to_string())?;
                if slice.is_empty() {
                    return Ok(None);
                }
                let target_kinds = kinds_from_cli(kind_cli.as_str())?;
                let found = find_unified_kinds(language, slice, &target_kinds);
                // Re-parsing a *full* loop node (assign …:node) makes that loop the tree root, so
                // the first DFS match would be the same loop again. Skip matches that cover the
                // entire sub-slice; keep looking for a strictly inner loop. (Assign …:body instead
                // of node if you only need “inside the braces” without this ambiguity.)
                let m = found.ok().and_then(|v| {
                    let slen = slice.len();
                    v.into_iter()
                        .find(|m| m.span.start_byte != 0 || m.span.end_byte != slen)
                });
                let Some(m) = m else {
                    return Ok(None);
                };
                current = offset_mapped_node(&m, sp.start_byte);
            }
            PipelineStep::Is { var, kind_cli } => {
                let sp = *bindings
                    .get(var)
                    .ok_or_else(|| format!("--step is: unknown variable {var:?}"))?;
                if sp.start_byte >= sp.end_byte {
                    return Ok(None);
                }
                let slice = source
                    .get(sp.start_byte..sp.end_byte)
                    .ok_or_else(|| "is: span out of range".to_string())?;
                if slice.is_empty() {
                    return Ok(None);
                }
                let target_kinds = kinds_from_cli(kind_cli.as_str())?;
                let Some(m) = match_root_as_unified(language, slice, &target_kinds) else {
                    return Ok(None);
                };
                current = offset_mapped_node(&m, sp.start_byte);
            }
            PipelineStep::Strip => {
                let sp = current.span;
                let slice = source
                    .get(sp.start_byte..sp.end_byte)
                    .ok_or_else(|| "strip: current span out of range".to_string())?;
                if slice.is_empty() {
                    return Ok(None);
                }
                let kinds = block_shape_kinds()?;
                let v = find_unified_kinds(language, slice, &kinds).map_err(|e: tree_lang::ParseError| {
                    format!("strip: parse in span: {e}")
                })?;
                let m = v
                    .into_iter()
                    .min_by_key(|m| (m.span.start_byte, m.span.end_byte));
                let Some(m) = m else {
                    return Ok(None);
                };
                current = offset_mapped_node(&m, sp.start_byte);
            }
            PipelineStep::Print { template } => {
                has_print = true;
                let line = render_for_pipeline(
                    path,
                    source,
                    language,
                    &current,
                    &bindings,
                    template,
                )?;
                println!("{line}");
            }
        }
    }

    Ok(Some(PipelineRunOutcome {
        need_default_print: !has_print,
    }))
}

fn render_for_pipeline(
    path: &Path,
    source: &str,
    language: Language,
    current: &MappedNode,
    bindings: &HashMap<String, Span>,
    template: &str,
) -> Result<String, String> {
    let (sl, sc) = byte_to_line_col(source, current.span.start_byte);
    let (el, ec) = byte_to_line_col(source, current.span.end_byte);
    let range = format!("{sl}:{sc}-{el}:{ec}");
    let start = format!("{sl}:{sc}");
    let end = format!("{el}:{ec}");
    let file = path.display().to_string();
    let kstr = format_kind(current.kind);
    let lang = language.as_cli_name();
    let node_text = span_text(source, current.span.start_byte, current.span.end_byte);
    let escaped_content = node_text.escape_default().to_string();
    let escaped_body = current
        .body
        .map(|b| {
            span_text(source, b.start_byte, b.end_byte)
                .escape_default()
                .to_string()
        })
        .unwrap_or_default();
    let sb = current.span.start_byte.to_string();
    let se = current.span.end_byte.to_string();
    let bsb = current
        .body
        .map(|b| b.start_byte.to_string())
        .unwrap_or_default();
    let bbe = current
        .body
        .map(|b| b.end_byte.to_string())
        .unwrap_or_default();

    let mut out = template.to_string();
    for (k, sp) in bindings {
        let raw = span_text(source, sp.start_byte, sp.end_byte);
        out = out.replace(&format!("{{{k}}}"), &raw);
    }
    Ok(render_template(
        &out,
        &file,
        &kstr,
        &start,
        &end,
        &range,
        "",
        &escaped_content,
        &escaped_body,
        lang,
        &sb,
        &se,
        &bsb,
        &bbe,
    ))
}
