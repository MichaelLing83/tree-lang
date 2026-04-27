use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use regex::Regex;
use walkdir::WalkDir;

use tree_lang::{
    for_each_subtree_node, format_kind, map_unified_node, parse, Language, MappedNode,
    TreeTraversal,
};

#[path = "../internal/pipeline.rs"]
mod pipeline;

/// Short help for `-h` (full step grammar is in [`STEP_ARG_LONG_HELP`]).
const STEP_ARG_HELP_SHORT: &str =
    "Pipeline step; repeat with --step (order matters). Use --help for full syntax.";

/// Long help for `--step` (shared by traversal commands; `find` is an alias of `dfs_preorder`).
const STEP_ARG_LONG_HELP: &str = r"Pipeline steps — repeat `--step`; order matters (node-centric).

  name=node | current | body | consequence | else | x.body | x.consequence | x.else
      `=node` is the per-hit find/traverse root; `=current` is the pipeline focus. `=body` / `=consequence`
      are relative to **current**; `=else` / `=alternative` is the if alternative.
      `=x.body` / `=x.consequence` / `=x.else` use another binding `x` (e.g. `b=n.body`).
      Does not move `current` by itself; later steps use the binding.

  x.is(KIND)          KIND uses unified kind strings (any, loop, branch:if, ...).
      Fails the pipeline if `x` is
      not one of those kinds. Sets `current` to `x` on success.

  x.has(KIND)         Like the old has: first strict inner unified hit in `x`'s span
      (skips the full-span root). Sets `current` to that node.

  x.first(A,B,…)     Union of unified kind lists; first DFS-preorder node under `x` (including
      `x` if it matches) whose kind is in that set. Sets `current` on success.

  name=x.first(A,B,…) Same as x.first(...), but also binds the result to `name`.
      The receiver may also be x.body, x.consequence, or x.else, e.g. c=x.else.first(loop).

  emit:TEMPLATE        One line of output mid-pipeline; `TEMPLATE` is like `--print-format`
      (dotted {x.y} for bindings plus legacy {type} etc. based on `current`).

`node` and `current` are always in scope. `--print` / `--print-format` after all steps
use the final `current` (dotted `name.field` bindings from after-pipeline state).

Example:
  --step 'n=node' --step 'b=n.body' --step 'b.has(loop)' --step 'emit:{b.type}'";

#[derive(Parser)]
#[command(
    name = "tree-lang",
    version,
    about = "Unified syntax search over source trees"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Shared flags for `dfs_preorder` / `find` / `dfs_postorder` / `bfs_ltr` / `bfs_rtl`.
#[derive(Args)]
struct TraverseArgs {
    /// One or more files or directories to read.
    paths: Vec<PathBuf>,
    /// Exclude paths whose string form matches this regular expression (repeatable).
    #[arg(short = 'e', long = "exclude", value_name = "REGEX")]
    exclude: Vec<String>,
    /// Target language: c, cpp, java, python, rust, auto (default: auto).
    #[arg(
        short = 'l',
        long = "language",
        value_name = "LANG",
        default_value = "auto"
    )]
    language: String,
    /// Print selected output fields: all or comma-separated fields.
    #[arg(
        long = "print",
        value_name = "FIELDS",
        num_args = 0..=1,
        default_missing_value = "all"
    )]
    print: Option<String>,
    /// Print using a format template.
    #[arg(long = "print-format", value_name = "TEMPLATE")]
    print_format: Option<String>,
    #[arg(
        short = 's',
        long = "step",
        value_name = "STEP",
        action = clap::ArgAction::Append,
        help = STEP_ARG_HELP_SHORT,
        long_help = STEP_ARG_LONG_HELP
    )]
    step: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Walk the full parse tree depth-first, preorder (node then each child, left to right),
    /// running `--step` (if any) from each node that is a unified construct, in that order.
    ///
    /// Alias: `find`.
    #[command(name = "dfs_preorder", visible_alias = "find")]
    DfsPreorder(TraverseArgs),
    /// Depth-first, postorder (all children, then the node; children left to right).
    #[command(name = "dfs_postorder")]
    DfsPostorder(TraverseArgs),
    /// Breadth-first, left-to-right within each level and child order in parse order.
    #[command(name = "bfs_ltr")]
    BfsLtr(TraverseArgs),
    /// Breadth-first, right-to-left siblings at each level (last parse-order child enqueued first).
    #[command(name = "bfs_rtl")]
    BfsRtl(TraverseArgs),
}

#[derive(Clone, Copy, Debug)]
enum LanguageSelector {
    Explicit(Language),
    Auto,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::DfsPreorder(a) => run_traverse(a, TreeTraversal::DfsPreorder),
        Command::DfsPostorder(a) => run_traverse(a, TreeTraversal::DfsPostorder),
        Command::BfsLtr(a) => run_traverse(a, TreeTraversal::BfsLtr),
        Command::BfsRtl(a) => run_traverse(a, TreeTraversal::BfsRtl),
    }
}

fn run_traverse(args: TraverseArgs, order: TreeTraversal) -> std::process::ExitCode {
    if args.paths.is_empty() {
        eprintln!("error: at least one path is required (or use '-' for stdin)");
        return std::process::ExitCode::from(2);
    }

    let print_fields = match args.print.as_deref() {
        Some(raw) => match parse_print_fields(raw) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("error: invalid --print value: {e}");
                return std::process::ExitCode::from(2);
            }
        },
        None => None,
    };
    if print_fields.is_some() && args.print_format.is_some() {
        eprintln!("error: --print and --print-format cannot be used together");
        return std::process::ExitCode::from(2);
    }

    let program: Option<pipeline::StepProgram> = if args.step.is_empty() {
        None
    } else {
        match pipeline::parse_steps(&args.step) {
            Ok(s) if s.is_empty() => None,
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("error: invalid --step: {e}");
                return std::process::ExitCode::from(2);
            }
        }
    };

    let language_selector = match parse_language_selector(&args.language) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let exclude: Vec<Regex> = match args.exclude.iter().map(|p| Regex::new(p)).collect() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid --exclude regex: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let mut files = Vec::new();
    let mut use_stdin = false;
    for root in &args.paths {
        if root.as_os_str() == "-" {
            use_stdin = true;
            continue;
        }
        let collected = match language_selector {
            LanguageSelector::Explicit(language) => collect_targets(root, language, &exclude),
            LanguageSelector::Auto => collect_targets_auto(root, &exclude),
        };
        match collected {
            Ok(mut v) => files.append(&mut v),
            Err(e) => {
                eprintln!("error: {}: {e}", root.display());
                return std::process::ExitCode::from(1);
            }
        }
    }

    files.sort();
    files.dedup();

    let mut had_error = false;
    if use_stdin {
        if matches!(language_selector, LanguageSelector::Auto) {
            eprintln!(
                "error: --language auto cannot be used with stdin; pass an explicit language"
            );
            return std::process::ExitCode::from(2);
        }
        let mut source = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("<stdin>: read error: {e}");
            return std::process::ExitCode::from(1);
        }
        let language = match language_selector {
            LanguageSelector::Explicit(l) => l,
            LanguageSelector::Auto => unreachable!(),
        };
        process_traverse_source(
            Path::new("<stdin>"),
            &source,
            language,
            order,
            print_fields.as_deref(),
            args.print_format.as_deref(),
            program.as_ref(),
            &mut had_error,
        );
    }

    for path in files {
        let language = match language_selector {
            LanguageSelector::Explicit(l) => l,
            LanguageSelector::Auto => {
                let Some(detected) = Language::detect_from_path(&path) else {
                    eprintln!(
                        "warning: {}: unsupported language for --language auto; skipping",
                        path.display()
                    );
                    continue;
                };
                detected
            }
        };
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: read error: {e}", path.display());
                had_error = true;
                continue;
            }
        };
        process_traverse_source(
            &path,
            &source,
            language,
            order,
            print_fields.as_deref(),
            args.print_format.as_deref(),
            program.as_ref(),
            &mut had_error,
        );
    }

    if had_error {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn process_traverse_source(
    path: &Path,
    source: &str,
    language: Language,
    order: TreeTraversal,
    print_fields: Option<&[PrintField]>,
    print_format: Option<&str>,
    pipeline: Option<&pipeline::StepProgram>,
    had_error: &mut bool,
) {
    let tree = match parse(language, source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}: parse error: {e}", path.display());
            *had_error = true;
            return;
        }
    };
    for_each_subtree_node(tree.root_node(), order, |node| {
        if let Some(m) = map_unified_node(language, &node) {
            if let Some(prog) = pipeline {
                match pipeline::run_unified_pipeline(
                    path,
                    source,
                    language,
                    tree.root_node(),
                    &m,
                    prog,
                ) {
                    Ok(Some(out)) if out.need_default_print => {
                        if let Some(ap) = &out.after {
                            print_unified_after(
                                path,
                                source,
                                language,
                                ap,
                                print_fields,
                                print_format,
                                had_error,
                            );
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("{}: {e}", path.display());
                        *had_error = true;
                    }
                }
            } else {
                print_unified(path, source, language, &m, print_fields, print_format);
            }
        }
    });
}

fn print_unified(
    path: &Path,
    source: &str,
    language: Language,
    m: &MappedNode,
    print_fields: Option<&[PrintField]>,
    print_format: Option<&str>,
) {
    let (sl, sc) = byte_to_line_col(source, m.span.start_byte);
    let (el, ec) = byte_to_line_col(source, m.span.end_byte);
    let escaped_body = m
        .body
        .map(|b| {
            span_text(source, b.start_byte, b.end_byte)
                .escape_default()
                .to_string()
        })
        .unwrap_or_default();
    let body_start_byte = m.body.map(|b| b.start_byte.to_string()).unwrap_or_default();
    let body_end_byte = m.body.map(|b| b.end_byte.to_string()).unwrap_or_default();
    print_match_line(
        path,
        language,
        &format_kind(m.kind),
        sl,
        sc,
        el,
        ec,
        m.span.start_byte,
        m.span.end_byte,
        None,
        span_text(source, m.span.start_byte, m.span.end_byte),
        &escaped_body,
        &body_start_byte,
        &body_end_byte,
        print_fields,
        print_format,
    );
}

fn print_unified_after(
    path: &Path,
    source: &str,
    language: Language,
    ap: &pipeline::AfterPipeline,
    print_fields: Option<&[PrintField]>,
    print_format: Option<&str>,
    had_error: &mut bool,
) {
    if let Some(t) = print_format {
        match pipeline::render_template_v2(path, source, language, ap, t) {
            Ok(line) => println!("{line}"),
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                *had_error = true;
            }
        }
        return;
    }
    print_unified(path, source, language, &ap.current, print_fields, None);
}

fn parse_language_selector(raw: &str) -> Result<LanguageSelector, String> {
    if raw.trim().eq_ignore_ascii_case("auto") {
        Ok(LanguageSelector::Auto)
    } else {
        raw.parse::<Language>().map(LanguageSelector::Explicit)
    }
}

fn collect_targets(
    root: &Path,
    language: Language,
    exclude: &[Regex],
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut out = Vec::new();
    let meta = fs::metadata(root)?;
    if meta.is_file() {
        if !is_excluded(root, exclude) {
            out.push(root.to_path_buf());
        }
        return Ok(out);
    }
    if meta.is_dir() {
        let exts: Vec<String> = language
            .source_extensions()
            .iter()
            .map(|e| e.to_ascii_lowercase())
            .collect();
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            if is_excluded(&path, exclude) {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| format!(".{}", s.to_ascii_lowercase()))
                .unwrap_or_default();
            if exts.contains(&ext) {
                out.push(path);
            }
        }
        return Ok(out);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "not a file or directory",
    ))
}

fn collect_targets_auto(root: &Path, exclude: &[Regex]) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut out = Vec::new();
    let meta = fs::metadata(root)?;
    if meta.is_file() {
        if !is_excluded(root, exclude) {
            out.push(root.to_path_buf());
        }
        return Ok(out);
    }
    if meta.is_dir() {
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            if is_excluded(&path, exclude) {
                continue;
            }
            out.push(path);
        }
        return Ok(out);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "not a file or directory",
    ))
}

fn is_excluded(path: &Path, exclude: &[Regex]) -> bool {
    let s = path.to_string_lossy();
    exclude.iter().any(|re| re.is_match(s.as_ref()))
}

/// 1-based line, 0-based UTF-8 column within that line (byte offset from line start).
pub(crate) fn byte_to_line_col(source: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(source.len());
    let prefix = &source[..byte];
    let line = prefix.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;
    let col = prefix
        .rsplit_once('\n')
        .map(|(_, line_prefix)| line_prefix.len())
        .unwrap_or(prefix.len());
    (line, col)
}

pub(crate) fn span_text(source: &str, start: usize, end: usize) -> String {
    let s = start.min(source.len());
    let e = end.min(source.len());
    if s >= e {
        String::new()
    } else {
        source[s..e].to_string()
    }
}

fn print_match_line(
    path: &Path,
    language: Language,
    kind: &str,
    sl: usize,
    sc: usize,
    el: usize,
    ec: usize,
    start_byte: usize,
    end_byte: usize,
    name: Option<&str>,
    node_text: String,
    escaped_body: &str,
    body_start_byte: &str,
    body_end_byte: &str,
    print_fields: Option<&[PrintField]>,
    print_format: Option<&str>,
) {
    let range = format!("{sl}:{sc}-{el}:{ec}");
    let start = format!("{sl}:{sc}");
    let end = format!("{el}:{ec}");
    let escaped_content = node_text.escape_default().to_string();
    let file = path.display().to_string();
    let lang = language.as_cli_name();
    let sb = start_byte.to_string();
    let se = end_byte.to_string();

    if let Some(template) = print_format {
        let rendered = render_template(
            template,
            &file,
            kind,
            &start,
            &end,
            &range,
            name.unwrap_or(""),
            &escaped_content,
            escaped_body,
            lang,
            &sb,
            &se,
            body_start_byte,
            body_end_byte,
        );
        println!("{rendered}");
        return;
    }

    if let Some(fields) = print_fields {
        let mut cols = Vec::new();
        for field in fields {
            match field {
                PrintField::File => cols.push(path.display().to_string()),
                PrintField::Type => cols.push(kind.to_string()),
                PrintField::Start => cols.push(start.clone()),
                PrintField::End => cols.push(end.clone()),
                PrintField::Content => cols.push(escaped_content.clone()),
                PrintField::Body => cols.push(escaped_body.to_string()),
                PrintField::Language => cols.push(lang.to_string()),
                PrintField::StartByte => cols.push(sb.clone()),
                PrintField::EndByte => cols.push(se.clone()),
                PrintField::BodyStartByte => cols.push(body_start_byte.to_string()),
                PrintField::BodyEndByte => cols.push(body_end_byte.to_string()),
            }
        }
        println!("{}", cols.join("\t"));
        return;
    }

    // Default output keeps the earlier shape for backward compatibility.
    if let Some(name) = name {
        println!("{}\t{}\t{}\t{}", path.display(), kind, range, name);
    } else {
        println!("{}\t{}\t{}", path.display(), kind, range);
    }
}

pub(crate) fn render_template(
    template: &str,
    file: &str,
    kind: &str,
    start: &str,
    end: &str,
    range: &str,
    name: &str,
    content: &str,
    body: &str,
    language: &str,
    start_byte: &str,
    end_byte: &str,
    body_start_byte: &str,
    body_end_byte: &str,
) -> String {
    template
        .replace("{file}", file)
        .replace("{type}", kind)
        .replace("{start}", start)
        .replace("{end}", end)
        .replace("{range}", range)
        .replace("{name}", name)
        .replace("{content}", content)
        .replace("{body}", body)
        .replace("{language}", language)
        .replace("{start_byte}", start_byte)
        .replace("{end_byte}", end_byte)
        .replace("{body_start_byte}", body_start_byte)
        .replace("{body_end_byte}", body_end_byte)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintField {
    File,
    Type,
    Start,
    End,
    Content,
    Body,
    Language,
    StartByte,
    EndByte,
    BodyStartByte,
    BodyEndByte,
}

fn parse_print_fields(raw: &str) -> Result<Vec<PrintField>, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized == "all" {
        return Ok(vec![
            PrintField::File,
            PrintField::Type,
            PrintField::Start,
            PrintField::End,
            PrintField::Content,
            PrintField::Body,
            PrintField::Language,
            PrintField::StartByte,
            PrintField::EndByte,
            PrintField::BodyStartByte,
            PrintField::BodyEndByte,
        ]);
    }
    if normalized.is_empty() {
        return Err("empty value".to_string());
    }

    let mut fields = Vec::new();
    for part in normalized.split(',') {
        let token = part.trim();
        let field = match token {
            "file" => PrintField::File,
            "type" => PrintField::Type,
            "start" => PrintField::Start,
            "end" => PrintField::End,
            "content" => PrintField::Content,
            "body" => PrintField::Body,
            "language" => PrintField::Language,
            "start_byte" => PrintField::StartByte,
            "end_byte" => PrintField::EndByte,
            "body_start_byte" => PrintField::BodyStartByte,
            "body_end_byte" => PrintField::BodyEndByte,
            _ => {
                return Err(format!(
                    "unknown field {token:?}; expected one of file,type,start,end,content,body,language,start_byte,end_byte,body_start_byte,body_end_byte or all"
                ));
            }
        };
        fields.push(field);
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use regex::Regex;

    use super::*;

    static TMP_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn parse_print_fields_accepts_all_and_subsets() {
        let all = parse_print_fields("all").expect("all fields");
        assert_eq!(all.len(), 11);
        assert_eq!(
            parse_print_fields(" file,TYPE,body_start_byte ").expect("subset"),
            vec![
                PrintField::File,
                PrintField::Type,
                PrintField::BodyStartByte
            ]
        );
    }

    #[test]
    fn parse_print_fields_rejects_empty_and_unknown_values() {
        assert_eq!(parse_print_fields("  ").expect_err("empty"), "empty value");
        let err = parse_print_fields("file,nope").expect_err("unknown field");
        assert!(err.contains("unknown field"));
    }

    #[test]
    fn render_template_replaces_every_placeholder() {
        let rendered = render_template(
            "{file}|{type}|{start}|{end}|{range}|{name}|{content}|{body}|{language}|{start_byte}|{end_byte}|{body_start_byte}|{body_end_byte}",
            "f.rs",
            "Loop(For)",
            "1:0",
            "1:8",
            "1:0-1:8",
            "f",
            "content",
            "body",
            "rust",
            "0",
            "8",
            "2",
            "7",
        );
        assert_eq!(
            rendered,
            "f.rs|Loop(For)|1:0|1:8|1:0-1:8|f|content|body|rust|0|8|2|7"
        );
    }

    #[test]
    fn byte_to_line_col_and_span_text_clamp_offsets() {
        let source = "α\nabc\n";
        assert_eq!(byte_to_line_col(source, 0), (1, 0));
        assert_eq!(byte_to_line_col(source, 3), (2, 0));
        assert_eq!(byte_to_line_col(source, 99), (3, 0));
        assert_eq!(span_text(source, 3, 6), "abc");
        assert_eq!(span_text(source, 6, 3), "");
        assert_eq!(span_text(source, 3, 99), "abc\n");
    }

    #[test]
    fn parse_language_selector_accepts_auto_and_languages() {
        assert!(matches!(
            parse_language_selector("auto").expect("auto"),
            LanguageSelector::Auto
        ));
        assert!(matches!(
            parse_language_selector("rust").expect("rust"),
            LanguageSelector::Explicit(Language::Rust)
        ));
        assert!(parse_language_selector("wat").is_err());
    }

    #[test]
    fn collect_targets_filters_by_language_and_exclude() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let rust = dir.join("lib.rs");
        let c = dir.join("lib.c");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).expect("create nested dir");
        let excluded = nested.join("skip.rs");
        fs::write(&rust, "fn main() {}\n").expect("write rust");
        fs::write(&c, "int main() {}\n").expect("write c");
        fs::write(&excluded, "fn skip() {}\n").expect("write excluded");

        let exclude = [Regex::new("skip").expect("regex")];
        let targets = collect_targets(&dir, Language::Rust, &exclude).expect("collect rust");
        assert_eq!(targets, vec![rust.clone()]);

        let auto_targets = collect_targets_auto(&dir, &exclude).expect("collect auto");
        assert!(auto_targets.contains(&rust));
        assert!(auto_targets.contains(&c));
        assert!(!auto_targets.contains(&excluded));
        assert!(is_excluded(Path::new("src/skip.rs"), &exclude));

        fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn collect_targets_handles_files_and_invalid_paths() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let rust = dir.join("main.rs");
        fs::write(&rust, "fn main() {}\n").expect("write rust");

        assert_eq!(
            collect_targets(&rust, Language::Rust, &[]).expect("collect file"),
            vec![rust.clone()]
        );
        assert_eq!(
            collect_targets_auto(&rust, &[]).expect("collect auto file"),
            vec![rust.clone()]
        );
        assert!(collect_targets(&dir.join("missing"), Language::Rust, &[]).is_err());

        fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tree_lang_bin_tests_{}_{}",
            std::process::id(),
            TMP_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
