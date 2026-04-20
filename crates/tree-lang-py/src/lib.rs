use std::fs;
use std::path::{Path, PathBuf};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use regex::Regex;
use ::tree_lang::{find_unified_kinds, Language, LoopKind, UnifiedKind};
use walkdir::WalkDir;

#[pyfunction]
fn supported_languages() -> Vec<&'static str> {
    vec!["c", "cpp", "java", "python", "rust"]
}

#[pyfunction]
fn supported_kinds() -> Vec<&'static str> {
    vec![
        "function_definition",
        "if",
        "loop",
        "loop:for",
        "loop:foreach",
        "loop:while",
        "loop:dowhile",
        "loop:infinite",
    ]
}

#[pyfunction]
fn find_in_source(
    py: Python<'_>,
    source: &str,
    language: &str,
    kind: &str,
) -> PyResult<Vec<PyObject>> {
    let language = parse_language(language)?;
    let kinds = parse_kind(kind)?;
    let matches = find_unified_kinds(language, source, &kinds)
        .map_err(|e| PyValueError::new_err(format!("parse failed: {e}")))?;
    let mut out = Vec::with_capacity(matches.len());
    for m in matches {
        let d = PyDict::new_bound(py);
        let (sl, sc) = byte_to_line_col(source, m.span.start_byte);
        let (el, ec) = byte_to_line_col(source, m.span.end_byte);
        d.set_item("kind", format_kind(m.kind))?;
        d.set_item("start_byte", m.span.start_byte)?;
        d.set_item("end_byte", m.span.end_byte)?;
        d.set_item("start_line", sl)?;
        d.set_item("start_col", sc)?;
        d.set_item("end_line", el)?;
        d.set_item("end_col", ec)?;
        d.set_item("content", span_text(source, m.span.start_byte, m.span.end_byte))?;
        out.push(d.into_any().unbind());
    }
    Ok(out)
}

#[pyfunction(signature = (paths, language, kind, exclude=None))]
fn find_in_paths(
    py: Python<'_>,
    paths: Vec<String>,
    language: &str,
    kind: &str,
    exclude: Option<Vec<String>>,
) -> PyResult<Vec<PyObject>> {
    if paths.is_empty() {
        return Err(PyValueError::new_err("paths must not be empty"));
    }
    let language = parse_language(language)?;
    let kinds = parse_kind(kind)?;
    let exclude = compile_excludes(exclude.unwrap_or_default())?;
    let files = collect_paths(&paths, language, &exclude)?;

    let mut out = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path).map_err(|e| {
            PyValueError::new_err(format!("failed reading {}: {e}", path.display()))
        })?;
        let matches = find_unified_kinds(language, &source, &kinds)
            .map_err(|e| PyValueError::new_err(format!("parse failed in {}: {e}", path.display())))?;
        for m in matches {
            let d = PyDict::new_bound(py);
            let (sl, sc) = byte_to_line_col(&source, m.span.start_byte);
            let (el, ec) = byte_to_line_col(&source, m.span.end_byte);
            d.set_item("file", path.to_string_lossy().to_string())?;
            d.set_item("kind", format_kind(m.kind))?;
            d.set_item("start_byte", m.span.start_byte)?;
            d.set_item("end_byte", m.span.end_byte)?;
            d.set_item("start_line", sl)?;
            d.set_item("start_col", sc)?;
            d.set_item("end_line", el)?;
            d.set_item("end_col", ec)?;
            d.set_item("content", span_text(&source, m.span.start_byte, m.span.end_byte))?;
            out.push(d.into_any().unbind());
        }
    }
    Ok(out)
}

#[pymodule(name = "tree_lang")]
fn tree_lang_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(supported_languages, m)?)?;
    m.add_function(wrap_pyfunction!(supported_kinds, m)?)?;
    m.add_function(wrap_pyfunction!(find_in_source, m)?)?;
    m.add_function(wrap_pyfunction!(find_in_paths, m)?)?;
    Ok(())
}

fn parse_language(raw: &str) -> PyResult<Language> {
    Language::parse_cli_name(raw).ok_or_else(|| {
        PyValueError::new_err(format!(
            "unknown language {raw:?}; expected one of: c, cpp, java, python, rust"
        ))
    })
}

fn parse_kind(raw: &str) -> PyResult<Vec<UnifiedKind>> {
    let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
    if let Some(sub) = normalized.strip_prefix("loop:") {
        let lk = match sub {
            "for" => LoopKind::For,
            "foreach" | "for_each" => LoopKind::ForEach,
            "while" => LoopKind::While,
            "dowhile" | "do_while" => LoopKind::DoWhile,
            "infinite" | "forever" => LoopKind::Infinite,
            _ => {
                return Err(PyValueError::new_err(format!(
                    "unknown loop subtype {sub:?}; expected for, foreach, while, dowhile, infinite"
                )))
            }
        };
        return Ok(vec![UnifiedKind::Loop(lk)]);
    }
    match normalized.as_str() {
        "functiondefinition" | "function_definition" | "fn" | "func" => {
            Ok(vec![UnifiedKind::FunctionDefinition])
        }
        "if" => Ok(vec![UnifiedKind::If]),
        "loop" => Ok(vec![
            UnifiedKind::Loop(LoopKind::For),
            UnifiedKind::Loop(LoopKind::ForEach),
            UnifiedKind::Loop(LoopKind::While),
            UnifiedKind::Loop(LoopKind::DoWhile),
            UnifiedKind::Loop(LoopKind::Infinite),
        ]),
        _ => Err(PyValueError::new_err(format!(
            "unknown kind {raw:?}; expected function_definition, if, loop, or loop:<subtype>"
        ))),
    }
}

fn compile_excludes(patterns: Vec<String>) -> PyResult<Vec<Regex>> {
    patterns
        .into_iter()
        .map(|p| {
            Regex::new(&p)
                .map_err(|e| PyValueError::new_err(format!("invalid exclude regex {p:?}: {e}")))
        })
        .collect()
}

fn collect_paths(
    paths: &[String],
    language: Language,
    exclude: &[Regex],
) -> PyResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    let exts: Vec<String> = language
        .source_extensions()
        .iter()
        .map(|e| e.to_ascii_lowercase())
        .collect();

    for p in paths {
        let path = Path::new(p);
        let meta = fs::metadata(path)
            .map_err(|e| PyValueError::new_err(format!("invalid path {}: {e}", path.display())))?;
        if meta.is_file() {
            if !is_excluded(path, exclude) {
                out.push(path.to_path_buf());
            }
            continue;
        }
        if meta.is_dir() {
            for entry in WalkDir::new(path).follow_links(false) {
                let entry = entry.map_err(|e| {
                    PyValueError::new_err(format!("walkdir error under {}: {e}", path.display()))
                })?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let f = entry.path();
                if is_excluded(f, exclude) {
                    continue;
                }
                let ext = f
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| format!(".{}", s.to_ascii_lowercase()))
                    .unwrap_or_default();
                if exts.contains(&ext) {
                    out.push(f.to_path_buf());
                }
            }
            continue;
        }
        return Err(PyValueError::new_err(format!(
            "path {} is neither file nor directory",
            path.display()
        )));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn is_excluded(path: &Path, exclude: &[Regex]) -> bool {
    let s = path.to_string_lossy();
    exclude.iter().any(|re| re.is_match(s.as_ref()))
}

fn format_kind(kind: UnifiedKind) -> String {
    match kind {
        UnifiedKind::FunctionDefinition => "FunctionDefinition".to_string(),
        UnifiedKind::If => "If".to_string(),
        UnifiedKind::Loop(k) => format!("Loop({k:?})"),
    }
}

fn byte_to_line_col(source: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(source.len());
    let prefix = &source[..byte];
    let line = prefix.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;
    let col = prefix
        .rsplit_once('\n')
        .map(|(_, line_prefix)| line_prefix.len())
        .unwrap_or(prefix.len());
    (line, col)
}

fn span_text(source: &str, start: usize, end: usize) -> String {
    let s = start.min(source.len());
    let e = end.min(source.len());
    if s >= e {
        String::new()
    } else {
        source[s..e].to_string()
    }
}
