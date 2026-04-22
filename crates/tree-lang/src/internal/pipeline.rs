//! Ordered find pipeline: ``--step`` with `assign|a`, `has|h`, `is|i`, `print|p`. See `find --help`.

use std::collections::HashMap;
use std::path::Path;

use tree_lang::{
    find_unified_kinds, match_root_as_unified, Language, MappedNode, Span, UnifiedKind,
};

use crate::{byte_to_line_col, format_kind, kinds_from_cli, render_template, span_text};

#[derive(Debug, Clone)]
pub enum PipelineStep {
    Assign { name: String, source: String },
    Has { var: String, kind_cli: String },
    /// Root of the parse of `var`'s span must be a unified node spanning the whole slice and match KIND.
    Is { var: String, kind_cli: String },
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
        ("print:", "print"),
        ("has:", "has"),
        ("is:", "is"),
        ("a:", "assign"),
        ("h:", "has"),
        ("p:", "print"),
        ("i:", "is"),
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
        let Some((step_kind, rest)) = split_step_prefix(s) else {
            return Err(format!(
                "unknown --step {s:?}; use assign:|a:, has:|h:, is:|i:, or print:|p: (see find --help)"
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

fn region_for_source(m: &MappedNode, from: &str) -> Result<Span, String> {
    let s = from.trim();
    match s {
        "node" | "node_span" => Ok(m.span),
        "body" => m
            .body
            .ok_or_else(|| "this node has no body field".to_string()),
        "consequence" => {
            if m.kind != UnifiedKind::If {
                return Err("--assign :consequence is only valid when the current node is an if".to_string());
            }
            m.body
                .ok_or_else(|| "if node has no consequence/body span".to_string())
        }
        _ => Err(format!(
            "unknown assign source {from:?}; use node, node_span, body, or consequence"
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
                let Some(m) = found.ok().and_then(|v| v.into_iter().next()) else {
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
