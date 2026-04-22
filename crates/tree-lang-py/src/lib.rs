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
        d.set_item("language", language.as_cli_name())?;
        d.set_item("start_byte", m.span.start_byte)?;
        d.set_item("end_byte", m.span.end_byte)?;
        d.set_item("start_line", sl)?;
        d.set_item("start_col", sc)?;
        d.set_item("end_line", el)?;
        d.set_item("end_col", ec)?;
        d.set_item("content", span_text(source, m.span.start_byte, m.span.end_byte))?;
        if let Some(b) = m.body {
            d.set_item("body", span_text(source, b.start_byte, b.end_byte))?;
        } else {
            d.set_item("body", py.None())?;
        }
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
            d.set_item("language", language.as_cli_name())?;
            d.set_item("start_byte", m.span.start_byte)?;
            d.set_item("end_byte", m.span.end_byte)?;
            d.set_item("start_line", sl)?;
            d.set_item("start_col", sc)?;
            d.set_item("end_line", el)?;
            d.set_item("end_col", ec)?;
            d.set_item("content", span_text(&source, m.span.start_byte, m.span.end_byte))?;
            if let Some(b) = m.body {
                d.set_item("body", span_text(&source, b.start_byte, b.end_byte))?;
            } else {
                d.set_item("body", py.None())?;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn parse_language_accepts_and_rejects() {
        assert!(parse_language("rust").is_ok());
        assert!(parse_language("RUST").is_ok());
        assert!(parse_language("nope").is_err());
    }

    #[test]
    fn parse_kind_covers_main_branches() {
        assert!(matches!(
            parse_kind("function_definition").unwrap().as_slice(),
            [UnifiedKind::FunctionDefinition]
        ));
        assert!(matches!(
            parse_kind("loop:for").unwrap().as_slice(),
            [UnifiedKind::Loop(LoopKind::For)]
        ));
        assert_eq!(parse_kind("loop").unwrap().len(), 5);
        assert!(parse_kind("loop:typo").is_err());
        assert!(parse_kind("unknown").is_err());
    }

    #[test]
    fn compile_excludes_rejects_bad_regex() {
        assert!(compile_excludes(vec!["(".to_string()]).is_err());
        assert_eq!(compile_excludes(vec![]).unwrap().len(), 0);
    }

    #[test]
    fn is_excluded_matches() {
        let re = Regex::new("secret").unwrap();
        assert!(is_excluded(Path::new("/tmp/secret.rs"), &[re]));
    }

    #[test]
    fn byte_to_line_col_and_span_text() {
        let src = "a\nbc\ndef";
        assert_eq!(byte_to_line_col(src, 0), (1, 0));
        assert_eq!(byte_to_line_col(src, 3), (2, 1));
        assert_eq!(span_text(src, 100, 200), "");
        assert_eq!(span_text(src, 2, 4), "bc");
    }

    #[test]
    fn format_kind_variants() {
        assert_eq!(
            format_kind(UnifiedKind::FunctionDefinition),
            "FunctionDefinition"
        );
        assert_eq!(format_kind(UnifiedKind::If), "If");
        assert!(format_kind(UnifiedKind::Loop(LoopKind::While)).starts_with("Loop"));
    }

    #[test]
    fn collect_paths_skips_non_file_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "tree_lang_py_collect_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();
        let rust_file = tmp.join("x.rs");
        fs::write(&rust_file, "fn x() {}\n").unwrap();
        let paths = vec![rust_file.to_string_lossy().to_string()];
        let got = collect_paths(&paths, Language::Rust, &[]).unwrap();
        assert_eq!(got, vec![rust_file]);
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn find_in_source_via_python_gil() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let out = find_in_source(py, "fn f() {}", "rust", "function_definition").unwrap();
            assert_eq!(out.len(), 1);
            use pyo3::types::PyAnyMethods;
            let obj = out[0].clone_ref(py).into_bound(py);
            let kind: String = obj.get_item("kind").unwrap().extract().unwrap();
            assert_eq!(kind, "FunctionDefinition");
        });
    }

    #[test]
    fn find_in_paths_empty_is_err() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            assert!(find_in_paths(py, vec![], "rust", "function_definition", None).is_err());
        });
    }

    #[test]
    fn find_in_paths_reads_temp_file() {
        pyo3::prepare_freethreaded_python();
        let tmp = std::env::temp_dir().join(format!(
            "tree_lang_py_paths_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("m.rs");
        fs::write(&f, "fn m() {}\n").unwrap();
        Python::with_gil(|py| {
            let out = find_in_paths(
                py,
                vec![f.to_string_lossy().to_string()],
                "rust",
                "function_definition",
                None,
            )
            .unwrap();
            assert_eq!(out.len(), 1);
        });
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn pymodule_registers_exports() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let m = PyModule::new_bound(py, "tree_lang").unwrap();
            tree_lang_py(&m).unwrap();
            assert!(m.getattr("find_in_source").is_ok());
        });
    }
}
