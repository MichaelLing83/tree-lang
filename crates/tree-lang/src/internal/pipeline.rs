//! ``--step`` / ``--print-format`` pipeline: node-centric statements on the current match and named bindings.
//!
//! One ``--step`` = one of:
//! - ``name=node|current|body|consequence`` and ``name=other.body|other.consequence`` — ``=node`` copies
//!   the per-hit **root**; ``=current`` the focus; ``=body`` / ``=consequence`` use **current**'s body or
//!   if-then span; ``x.body`` / ``x.consequence`` use another binding's span. Does *not* move ``current``
//!   by itself; updates ``name`` in the binding map.
//! - ``x.is(kinds)`` — ``kinds`` is the same grammar as ``-k`` (e.g. ``loop``, ``loop:for``, ``branch:match``).
//!   Succeeds if the unified kind of binding ``x`` is among those; sets ``current`` to ``x``.
//! - ``x.has(kinds)`` — within ``x``'s span, first *strict* inner unified hit (re-parse, skip root
//!   filling the whole slice, same as old ``has``), sets ``current``.
//! - ``x.first(a,b,…)`` — each argument is a ``-k`` kind spec; their union is the set of ``UnifiedKind``;
//!   DFS (preorder) in the file tree on ``x``'s span, *including* ``x`` if it matches. Sets ``current`` to
//!   the first hit.
//! - ``emit:…`` — one line, same template rules as ``--print-format`` (dotted and legacy).
//!
//! Bindings: ``node`` and ``current`` are always in scope. ``node`` = per-hit root (immutable).
//! ``current`` moves on ``is`` / ``has`` / ``first`` / after a successful chain.

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::Node;
use tree_lang::{
    find_unified_kinds, format_kind, kinds_from_cli, map_unified_node, parse, BranchKind, Language,
    MappedNode, Span, UnifiedKind,
};

use crate::{byte_to_line_col, render_template, span_text};

/// Program is a list of one-line statements, one per ``--step``.
#[derive(Debug, Clone)]
pub struct StepProgram {
    steps: Vec<StepStmt>,
}

impl StepProgram {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Debug, Clone)]
enum StepStmt {
    Assign {
        name: String,
        source: AssignSource,
    },
    Is { on: String, kind: String },
    Has { on: String, kind: String },
    First { on: String, args: Vec<String> },
    Emit { template: String },
}

/// Which field to take in an ``=…body`` / ``=…consequence`` assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelBody {
    Body,
    Consequence,
}

#[derive(Debug, Clone)]
enum AssignSource {
    /// Per-hit root (find / traverse), not the moving [AfterPipeline::current].
    Node,
    /// The pipeline focus at this step (moves on ``is`` / ``has`` / ``first``).
    Current,
    /// ``=body`` or ``=consequence`` (relative to **current**).
    Relative(RelBody),
    /// ``=x.body`` or ``=x.consequence`` (relative to binding ``x``).
    OnBinding { on: String, part: RelBody },
}

pub struct PipelineRunOutcome {
    pub need_default_print: bool,
    /// If steps ran, last ``current``; else none (caller can use the root match only).
    pub after: Option<AfterPipeline>,
}

pub struct AfterPipeline {
    pub node: MappedNode,
    pub current: MappedNode,
    pub bindings: HashMap<String, MappedNode>,
}

// --- public API -------------------------------------------------------------

pub fn parse_steps(step_strings: &[String]) -> Result<StepProgram, String> {
    let mut v = Vec::new();
    for s in step_strings {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        v.push(parse_one_step(s)?);
    }
    Ok(StepProgram { steps: v })
}

/// Run pipeline. On success, ``root`` is the per-hit root; **current** and bindings evolve from it.
/// Default print (when no ``emit:``) uses the **root**'s one-line or ``--print-format`` with ``AfterPipeline``.
pub fn run_unified_pipeline(
    path: &Path,
    source: &str,
    language: Language,
    root: &MappedNode,
    program: &StepProgram,
) -> Result<Option<PipelineRunOutcome>, String> {
    if program.is_empty() {
        return Ok(Some(PipelineRunOutcome {
            need_default_print: true,
            after: Some(AfterPipeline {
                node: *root,
                current: *root,
                bindings: default_bindings(*root),
            }),
        }));
    }
    let tree = parse(language, source)
        .map_err(|e: tree_lang::ParseError| format!("parse file: {e}"))?;
    let file_root = tree.root_node();
    let mut current = *root;
    let node_root = *root;
    let mut bindings = default_bindings(*root);
    let mut has_emit = false;

    for s in &program.steps {
        match s {
            StepStmt::Assign { name, source: src } => {
                let m = match &src {
                    AssignSource::Node => node_root,
                    AssignSource::Current => current,
                    AssignSource::Relative(part) => {
                        match assign_body_or_consequence(
                            language,
                            file_root,
                            &current,
                            *part,
                        ) {
                            BodyAssign::Ok(m) => m,
                            BodyAssign::NoMatch => {
                                return Ok(None);
                            }
                        }
                    }
                    AssignSource::OnBinding { on, part } => {
                        let base = *bindings
                            .get(on)
                            .ok_or_else(|| format!("unknown name {on:?} in =…body / =…consequence"))?;
                        match assign_body_or_consequence(language, file_root, &base, *part) {
                            BodyAssign::Ok(m) => m,
                            BodyAssign::NoMatch => {
                                return Ok(None);
                            }
                        }
                    }
                };
                bindings.insert(name.clone(), m);
            }
            StepStmt::Is { on, kind } => {
                let x = *bindings
                    .get(on)
                    .ok_or_else(|| format!("unknown name {on:?} in is()"))?;
                let allowed = kinds_from_cli(kind.as_str())?;
                if !allowed.contains(&x.kind) {
                    return Ok(None);
                }
                current = x;
                bindings.insert("current".to_string(), current);
            }
            StepStmt::Has { on, kind } => {
                let sp = bindings
                    .get(on)
                    .ok_or_else(|| format!("unknown name {on:?} in has()"))?
                    .span;
                let slice = source
                    .get(sp.start_byte..sp.end_byte)
                    .ok_or_else(|| "has: span out of range".to_string())?;
                if slice.is_empty() {
                    return Ok(None);
                }
                let target = kinds_from_cli(kind.as_str())?;
                let found = find_unified_kinds(language, slice, &target);
                let m = found.ok().and_then(|v| {
                    let slen = slice.len();
                    v.into_iter()
                        .find(|m| m.span.start_byte != 0 || m.span.end_byte != slen)
                });
                let Some(m) = m else { return Ok(None) };
                current = offset(&m, sp.start_byte);
                bindings.insert("current".to_string(), current);
            }
            StepStmt::First { on, args } => {
                let base = *bindings
                    .get(on)
                    .ok_or_else(|| format!("unknown name {on:?} in first()"))?;
                let mut u: Vec<UnifiedKind> = vec![];
                for a in args {
                    for k in kinds_from_cli(a.trim())? {
                        if !u.contains(&k) {
                            u.push(k);
                        }
                    }
                }
                let Some(n) = ts_node_exact(file_root, base.span.start_byte, base.span.end_byte) else {
                    return Err("first: span not in tree".to_string());
                };
                let m = first_preorder_match(language, n, &u)?;
                let Some(m) = m else {
                    return Ok(None);
                };
                current = m;
                bindings.insert("current".to_string(), current);
            }
            StepStmt::Emit { template } => {
                has_emit = true;
                let ap = AfterPipeline {
                    node: node_root,
                    current,
                    bindings: bindings.clone(),
                };
                let line = render_template_v2(
                    path,
                    source,
                    language,
                    &ap,
                    template,
                )?;
                println!("{line}");
            }
        }
    }

    Ok(Some(PipelineRunOutcome {
        need_default_print: !has_emit,
        after: Some(AfterPipeline {
            node: node_root,
            current,
            bindings,
        }),
    }))
}

// --- print / template (shared by emit and by bin) ---------------------------

/// Expand ``{name.field}`` and legacy ``{type}`` etc. using [AfterPipeline] (or root-only: pass same node for both).
pub fn render_template_v2(
    path: &Path,
    source: &str,
    language: Language,
    ap: &AfterPipeline,
    template: &str,
) -> Result<String, String> {
    // First: dotted placeholders
    let mut out = String::new();
    let mut i = 0;
    let bytes = template.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = find_closing_brace(&template[i + 1..]) {
                let j = i + 1 + end;
                let inner = &template[i + 1..i + 1 + end];
                if inner.contains('.') {
                    let (a, b) = inner.rsplit_once('.').ok_or("bad {a.b} placeholder")?;
                    let val = field_of_binding(path, source, language, a, b.trim(), ap)?;
                    out.push_str(&val);
                } else {
                    // pass through for second phase (legacy single token)
                    out.push('{');
                    out.push_str(inner);
                    out.push('}');
                }
                i = j + 1;
                continue;
            }
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    // Legacy: use *current* for the standard set
    let c = &ap.current;
    let (sl, sc) = byte_to_line_col(source, c.span.start_byte);
    let (el, ec) = byte_to_line_col(source, c.span.end_byte);
    let range = format!("{sl}:{sc}-{el}:{ec}");
    let start = format!("{sl}:{sc}");
    let end = format!("{el}:{ec}");
    let file = path.display().to_string();
    let kstr = format_kind(c.kind);
    let lang = language.as_cli_name();
    let ecnt = span_text(source, c.span.start_byte, c.span.end_byte).escape_default().to_string();
    let body = c
        .body
        .map(|b| {
            span_text(source, b.start_byte, b.end_byte)
                .escape_default()
                .to_string()
        })
        .unwrap_or_default();
    let sb = c.span.start_byte.to_string();
    let se = c.span.end_byte.to_string();
    let bsb = c
        .body
        .map(|b| b.start_byte.to_string())
        .unwrap_or_default();
    let bbe = c.body.map(|b| b.end_byte.to_string()).unwrap_or_default();
    // also substitute raw binding names: {bname} = escaped content
    let mut t = out;
    for (name, m) in &ap.bindings {
        let content = span_text(source, m.span.start_byte, m.span.end_byte)
            .escape_default()
            .to_string();
        t = t.replace(&format!("{{{name}}}"), &content);
    }
    t = t.replace("{node}", &span_text(source, ap.node.span.start_byte, ap.node.span.end_byte).escape_default().to_string());
    t = t.replace(
        "{current}",
        &span_text(source, ap.current.span.start_byte, ap.current.span.end_byte)
            .escape_default()
            .to_string(),
    );
    Ok(render_template(
        &t,
        &file,
        &kstr,
        &start,
        &end,
        &range,
        "",
        &ecnt,
        &body,
        lang,
        &sb,
        &se,
        &bsb,
        &bbe,
    ))
}

fn find_closing_brace(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'}' {
            return Some(i);
        }
    }
    None
}

fn field_of_binding(
    path: &Path,
    source: &str,
    language: Language,
    name: &str,
    field: &str,
    ap: &AfterPipeline,
) -> Result<String, String> {
    let m = *ap
        .bindings
        .get(name)
        .or_else(|| {
            if name == "node" {
                Some(&ap.node)
            } else if name == "current" {
                Some(&ap.current)
            } else {
                None
            }
        })
        .ok_or_else(|| format!("unknown node {name} in template"))?;
    field_string(path, source, language, &m, field)
}

fn field_string(
    path: &Path,
    source: &str,
    language: Language,
    m: &MappedNode,
    field: &str,
) -> Result<String, String> {
    let (sl, sc) = byte_to_line_col(source, m.span.start_byte);
    let (el, ec) = byte_to_line_col(source, m.span.end_byte);
    let range = format!("{sl}:{sc}-{el}:{ec}");
    let start = format!("{sl}:{sc}");
    let end = format!("{el}:{ec}");
    match field {
        "type" => Ok(format_kind(m.kind)),
        "start" => Ok(start),
        "end" => Ok(end),
        "range" => Ok(range),
        "content" => Ok(span_text(source, m.span.start_byte, m.span.end_byte)
            .escape_default()
            .to_string()),
        "body" => {
            let t = m
                .body
                .map(|b| {
                    span_text(source, b.start_byte, b.end_byte)
                        .escape_default()
                        .to_string()
                })
                .unwrap_or_default();
            Ok(t)
        }
        "language" => Ok(language.as_cli_name().to_string()),
        "file" => Ok(path.display().to_string()),
        "start_byte" => Ok(m.span.start_byte.to_string()),
        "end_byte" => Ok(m.span.end_byte.to_string()),
        "body_start_byte" | "body_start" => {
            if let Some(b) = m.body {
                Ok(b.start_byte.to_string())
            } else {
                Ok(String::new())
            }
        }
        "body_end_byte" | "body_end" => {
            if let Some(b) = m.body {
                Ok(b.end_byte.to_string())
            } else {
                Ok(String::new())
            }
        }
        _ => Err(format!("unknown field {field} on node; use file,type,start,end,range,content,body,language,...")),
    }
}

// --- internal ---------------------------------------------------------------

fn default_bindings(root: MappedNode) -> HashMap<String, MappedNode> {
    let mut b = HashMap::new();
    b.insert("node".to_string(), root);
    b.insert("current".to_string(), root);
    b
}

fn offset(m: &MappedNode, off: usize) -> MappedNode {
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

fn ts_node_exact<'a>(node: Node<'a>, start: usize, end: usize) -> Option<Node<'a>> {
    let r = node.range();
    if r.start_byte == start && r.end_byte == end {
        return Some(node);
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if let Some(n) = ts_node_exact(child, start, end) {
            return Some(n);
        }
    }
    None
}

/// First preorder node (including n) with unified kind in `set`
fn first_preorder_match(
    language: Language,
    n: Node<'_>,
    set: &[UnifiedKind],
) -> Result<Option<MappedNode>, String> {
    if let Some(m) = map_unified_node(language, &n) {
        if set.contains(&m.kind) {
            return Ok(Some(m));
        }
    }
    let mut c = n.walk();
    for child in n.children(&mut c) {
        if let Some(m) = first_preorder_match(language, child, set)? {
            return Ok(Some(m));
        }
    }
    Ok(None)
}

/// First tree-sitter node in DFS preorder (including `n`) that maps to a [MappedNode].
fn first_unified_mapped_in_subtree(language: Language, n: Node<'_>) -> Option<MappedNode> {
    if let Some(m) = map_unified_node(language, &n) {
        return Some(m);
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        if let Some(m) = first_unified_mapped_in_subtree(language, ch) {
            return Some(m);
        }
    }
    None
}

/// `NoMatch` means: skip this hit, not a file-level error.
enum BodyAssign {
    Ok(MappedNode),
    NoMatch,
}

fn assign_body_or_consequence(
    language: Language,
    file_root: Node<'_>,
    base: &MappedNode,
    which: RelBody,
) -> BodyAssign {
    let span = match which {
        RelBody::Body => {
            if let Some(b) = base.body {
                b
            } else {
                return BodyAssign::NoMatch;
            }
        }
        RelBody::Consequence => {
            if base.kind != UnifiedKind::Branch(BranchKind::If) {
                return BodyAssign::NoMatch;
            }
            if let Some(c) = base.body {
                c
            } else {
                return BodyAssign::NoMatch;
            }
        }
    };
    if span.start_byte >= span.end_byte {
        return BodyAssign::NoMatch;
    }
    let Some(n) = ts_node_exact(file_root, span.start_byte, span.end_byte) else {
        // Should not happen if `base` and `file_root` are consistent; treat as skip.
        return BodyAssign::NoMatch;
    };
    if let Some(m) = map_unified_node(language, &n) {
        return BodyAssign::Ok(m);
    }
    if let Some(inner) = first_unified_mapped_in_subtree(language, n) {
        return BodyAssign::Ok(MappedNode {
            span,
            kind: inner.kind,
            body: None,
        });
    }
    BodyAssign::NoMatch
}

fn parse_one_step(s: &str) -> Result<StepStmt, String> {
    let s = s.trim();
    if s.starts_with("emit:") {
        return Ok(StepStmt::Emit {
            template: s[5..].to_string(),
        });
    }
    // assign: name=node|current|body|consequence|x.body|x.consequence
    if let Some(eq) = s.find('=') {
        let name = s[..eq].trim();
        let rhs = s[eq + 1..].trim();
        if name.is_empty() {
            return Err("assign: empty name".to_string());
        }
        let source = if let Some((on, field)) = rhs.rsplit_once('.') {
            let on = on.trim();
            let field = field.trim();
            if on.is_empty() {
                return Err("assign: empty name before . in x.body / x.consequence".to_string());
            }
            let part = match field {
                "body" => RelBody::Body,
                "consequence" => RelBody::Consequence,
                f => {
                    return Err(format!(
                        "assign: after . use `body` or `consequence`, not {f:?} (in {rhs:?})"
                    ));
                }
            };
            AssignSource::OnBinding {
                on: on.to_string(),
                part,
            }
        } else {
            match rhs {
                "node" => AssignSource::Node,
                "current" => AssignSource::Current,
                "body" => AssignSource::Relative(RelBody::Body),
                "consequence" => AssignSource::Relative(RelBody::Consequence),
                o => {
                    return Err(format!(
                        "assign: use node, current, body, consequence, or x.body / x.consequence, not {o:?}"
                    ));
                }
            }
        };
        return Ok(StepStmt::Assign {
            name: name.to_string(),
            source,
        });
    }
    let mut best: Option<(usize, &'static str)> = None;
    for (pat, tag) in [(".is(", "is"), (".has(", "has"), (".first(", "first")] {
        if let Some(p) = s.find(pat) {
            if best.map_or(true, |(bp, _)| p < bp) {
                best = Some((p, tag));
            }
        }
    }
    if let Some((p, tag)) = best {
        let on = s[..p].trim();
        if on.is_empty() {
            return Err(format!("bad method call: {s:?}"));
        }
        let pat = match tag {
            "is" => ".is(",
            "has" => ".has(",
            "first" => ".first(",
            _ => unreachable!(),
        };
        let from = p + pat.len() - 1; // index of '('
        if s.as_bytes().get(from) != Some(&b'(') {
            return Err(format!("expected '(' after {tag} in {s:?}"));
        }
        let inner = extract_parens(s, from)?;
        return match tag {
            "is" => {
                if inner.is_empty() {
                    return Err("is() needs a kind string".to_string());
                }
                Ok(StepStmt::Is {
                    on: on.to_string(),
                    kind: inner.to_string(),
                })
            }
            "has" => {
                if inner.is_empty() {
                    return Err("has() needs a kind string".to_string());
                }
                Ok(StepStmt::Has {
                    on: on.to_string(),
                    kind: inner.to_string(),
                })
            }
            "first" => {
                let args = split_comma_list(inner);
                if args.is_empty() {
                    return Err("first() needs at least one kind".to_string());
                }
                Ok(StepStmt::First {
                    on: on.to_string(),
                    args,
                })
            }
            _ => unreachable!(),
        };
    }
    Err(format!(
        "unrecognized --step: {s:?}. Try name=node, name=x.body, x.is(kinds), x.has(kinds), x.first(a,b), emit:..."
    ))
}

/// s[from] is '('; return string inside the matching paren
fn extract_parens(s: &str, from: usize) -> Result<&str, String> {
    let b = s.as_bytes();
    if b.get(from) != Some(&b'(') {
        return Err("expected (".to_string());
    }
    let mut d = 0i32;
    for i in from..b.len() {
        match b[i] {
            b'(' => d += 1,
            b')' => {
                d -= 1;
                if d == 0 {
                    return std::str::from_utf8(&b[from + 1..i])
                        .map_err(|_| "utf8".to_string())
                        .map(|r| {
                            r // inner
                        });
                }
            }
            _ => {}
        }
    }
    Err("unclosed (".to_string())
}

/// Split on commas at paren depth 0
fn split_comma_list(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut d = 0i32;
    let mut start = 0;
    let mut out = Vec::new();
    for i in 0..=b.len() {
        let c = b.get(i).map(|&x| x);
        if let Some(x) = c {
            match x {
                b'(' | b'[' | b'{' => d += 1,
                b')' | b']' | b'}' => d -= 1,
                b',' if d == 0 => {
                    out.push(s[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        } else {
            if start < s.len() {
                out.push(s[start..i].trim().to_string());
            }
        }
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}