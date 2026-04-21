use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use regex::Regex;
use walkdir::WalkDir;

use tree_lang::{find_function_definitions, find_unified_kinds, Language, LoopKind, UnifiedKind};

#[derive(Parser)]
#[command(name = "tree-lang", version, about = "Unified syntax search over source trees")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Find unified syntax constructs in source files.
    Find(FindArgs),
}

#[derive(Args)]
struct FindArgs {
    /// One or more files or directories to search.
    paths: Vec<PathBuf>,
    /// Exclude paths whose string form matches this regular expression (repeatable).
    #[arg(short = 'e', long = "exclude", value_name = "REGEX")]
    exclude: Vec<String>,
    /// Target language: c, cpp, java, python, rust.
    #[arg(short = 'l', long = "language", value_name = "LANG")]
    language: String,
    /// Unified syntax kind: function_definition, if, loop, or loop:for / loop:while / …
    #[arg(short = 'k', long = "kind", value_name = "KIND")]
    kind: String,
    /// Match the name of found structures (when the selected kind has names, e.g. functions).
    #[arg(short = 'n', long = "name", value_name = "REGEX")]
    name: Option<String>,
    /// Match any function parameter name by regular expression (repeatable).
    #[arg(short = 'p', long = "param-name", value_name = "REGEX")]
    param_name: Vec<String>,
    /// Match any function parameter type by regular expression (repeatable).
    #[arg(short = 't', long = "param-type", value_name = "REGEX")]
    param_type: Vec<String>,
    /// Match function return type by regular expression (repeatable).
    #[arg(short = 'r', long = "return-type", value_name = "REGEX")]
    return_type: Vec<String>,
    /// Match function parameter name at index: <IDX:REGEX> (repeatable, 0-indexed).
    #[arg(long = "param-name-at", value_name = "IDX:REGEX")]
    param_name_at: Vec<String>,
    /// Match function parameter type at index: <IDX:REGEX> (repeatable, 0-indexed).
    #[arg(long = "param-type-at", value_name = "IDX:REGEX")]
    param_type_at: Vec<String>,
    /// Print selected output fields: all or comma-separated fields.
    /// Fields: file,type,start,end,content
    /// `--print` (without value) is equivalent to `--print all`.
    #[arg(
        long = "print",
        value_name = "FIELDS",
        num_args = 0..=1,
        default_missing_value = "all"
    )]
    print: Option<String>,
    /// Print using a format template.
    /// Supported placeholders: {file},{type},{start},{end},{range},{name},{content}
    #[arg(long = "print-format", value_name = "TEMPLATE")]
    print_format: Option<String>,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Find(args) => run_find(args),
    }
}

fn run_find(args: FindArgs) -> std::process::ExitCode {
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

    let language: Language = match args.language.parse() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let kinds = match kinds_from_cli(&args.kind) {
        Ok(k) => k,
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
    let name_regex = match args.name.as_deref() {
        Some(p) => match Regex::new(p) {
            Ok(re) => Some(re),
            Err(e) => {
                eprintln!("error: invalid --name regex: {e}");
                return std::process::ExitCode::from(2);
            }
        },
        None => None,
    };
    let param_name_regexes: Vec<Regex> = match args.param_name.iter().map(|p| Regex::new(p)).collect() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid --param-name regex: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let param_type_regexes: Vec<Regex> = match args.param_type.iter().map(|p| Regex::new(p)).collect() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid --param-type regex: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let return_type_regexes: Vec<Regex> = match args.return_type.iter().map(|p| Regex::new(p)).collect() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid --return-type regex: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let indexed_param_name = match parse_indexed_regexes(&args.param_name_at, "--param-name-at") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let indexed_param_type = match parse_indexed_regexes(&args.param_type_at, "--param-type-at") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let uses_function_filters = name_regex.is_some()
        || !param_name_regexes.is_empty()
        || !param_type_regexes.is_empty()
        || !return_type_regexes.is_empty()
        || !indexed_param_name.is_empty()
        || !indexed_param_type.is_empty();
    if uses_function_filters && !supports_name_filter(&kinds) {
        eprintln!(
            "error: --name/--param-name/--param-type/--return-type/--param-name-at/--param-type-at are currently supported only with --kind function_definition"
        );
        return std::process::ExitCode::from(2);
    }

    let mut files = Vec::new();
    let mut use_stdin = false;
    for root in &args.paths {
        if root.as_os_str() == "-" {
            use_stdin = true;
            continue;
        }
        match collect_targets(root, language, &exclude) {
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
        let mut source = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("<stdin>: read error: {e}");
            return std::process::ExitCode::from(1);
        }
        process_source(
            Path::new("<stdin>"),
            &source,
            language,
            &kinds,
            uses_function_filters,
            name_regex.as_ref(),
            &param_name_regexes,
            &param_type_regexes,
            &return_type_regexes,
            &indexed_param_name,
            &indexed_param_type,
            print_fields.as_deref(),
            args.print_format.as_deref(),
            &mut had_error,
        );
    }

    for path in files {
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: read error: {e}", path.display());
                had_error = true;
                continue;
            }
        };
        process_source(
            &path,
            &source,
            language,
            &kinds,
            uses_function_filters,
            name_regex.as_ref(),
            &param_name_regexes,
            &param_type_regexes,
            &return_type_regexes,
            &indexed_param_name,
            &indexed_param_type,
            print_fields.as_deref(),
            args.print_format.as_deref(),
            &mut had_error,
        );
    }

    if had_error {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}

#[allow(clippy::too_many_arguments)]
fn process_source(
    path: &Path,
    source: &str,
    language: Language,
    kinds: &[UnifiedKind],
    uses_function_filters: bool,
    name_regex: Option<&Regex>,
    param_name_regexes: &[Regex],
    param_type_regexes: &[Regex],
    return_type_regexes: &[Regex],
    indexed_param_name: &[IndexedRegex],
    indexed_param_type: &[IndexedRegex],
    print_fields: Option<&[PrintField]>,
    print_format: Option<&str>,
    had_error: &mut bool,
) {
    if uses_function_filters {
        let functions = match find_function_definitions(language, source) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{}: parse error: {e}", path.display());
                *had_error = true;
                return;
            }
        };
        for f in functions.into_iter().filter(|f| {
            function_matches(
                f,
                name_regex,
                param_name_regexes,
                param_type_regexes,
                return_type_regexes,
                indexed_param_name,
                indexed_param_type,
            )
        }) {
            let (sl, sc) = byte_to_line_col(source, f.span.start_byte);
            let (el, ec) = byte_to_line_col(source, f.span.end_byte);
            print_match_line(
                path,
                &format_kind(UnifiedKind::FunctionDefinition),
                sl,
                sc,
                el,
                ec,
                Some(&f.name),
                span_text(source, f.span.start_byte, f.span.end_byte),
                print_fields,
                print_format,
            );
        }
    } else {
        let matches = match find_unified_kinds(language, source, kinds) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}: parse error: {e}", path.display());
                *had_error = true;
                return;
            }
        };
        for m in matches {
            let (sl, sc) = byte_to_line_col(source, m.span.start_byte);
            let (el, ec) = byte_to_line_col(source, m.span.end_byte);
            print_match_line(
                path,
                &format_kind(m.kind),
                sl,
                sc,
                el,
                ec,
                None,
                span_text(source, m.span.start_byte, m.span.end_byte),
                print_fields,
                print_format,
            );
        }
    }
}

fn supports_name_filter(kinds: &[UnifiedKind]) -> bool {
    kinds.len() == 1 && kinds[0] == UnifiedKind::FunctionDefinition
}

#[derive(Debug)]
struct IndexedRegex {
    idx: usize,
    regex: Regex,
}

fn parse_indexed_regexes(values: &[String], flag: &str) -> Result<Vec<IndexedRegex>, String> {
    values
        .iter()
        .map(|raw| {
            let (idx_s, regex_s) = raw
                .split_once(':')
                .ok_or_else(|| format!("{flag} expects IDX:REGEX, got {raw:?}"))?;
            let idx = idx_s
                .parse::<usize>()
                .map_err(|_| format!("{flag} index must be a non-negative integer, got {idx_s:?}"))?;
            let regex = Regex::new(regex_s)
                .map_err(|e| format!("invalid regex in {flag} ({raw:?}): {e}"))?;
            Ok(IndexedRegex { idx, regex })
        })
        .collect()
}

fn function_matches(
    f: &tree_lang::FunctionDefinitionNode,
    name: Option<&Regex>,
    param_name: &[Regex],
    param_type: &[Regex],
    return_type: &[Regex],
    param_name_at: &[IndexedRegex],
    param_type_at: &[IndexedRegex],
) -> bool {
    if let Some(name_re) = name {
        if !name_re.is_match(&f.name) {
            return false;
        }
    }
    for re in param_name {
        if !f.parameters.iter().any(|p| re.is_match(&p.name)) {
            return false;
        }
    }
    for re in param_type {
        if !f
            .parameters
            .iter()
            .any(|p| re.is_match(p.ty.as_deref().unwrap_or("")))
        {
            return false;
        }
    }
    for re in return_type {
        if !re.is_match(f.return_type.as_deref().unwrap_or("")) {
            return false;
        }
    }
    for rule in param_name_at {
        let Some(param) = f.parameters.get(rule.idx) else {
            return false;
        };
        if !rule.regex.is_match(&param.name) {
            return false;
        }
    }
    for rule in param_type_at {
        let Some(param) = f.parameters.get(rule.idx) else {
            return false;
        };
        if !rule.regex.is_match(param.ty.as_deref().unwrap_or("")) {
            return false;
        }
    }
    true
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

fn is_excluded(path: &Path, exclude: &[Regex]) -> bool {
    let s = path.to_string_lossy();
    exclude.iter().any(|re| re.is_match(s.as_ref()))
}

fn kinds_from_cli(s: &str) -> Result<Vec<UnifiedKind>, String> {
    let normalized = s.trim().to_ascii_lowercase().replace('-', "_");
    if let Some(sub) = normalized.strip_prefix("loop:") {
        let lk = match sub {
            "for" => LoopKind::For,
            "foreach" | "for_each" => LoopKind::ForEach,
            "while" => LoopKind::While,
            "dowhile" | "do_while" => LoopKind::DoWhile,
            "infinite" | "forever" => LoopKind::Infinite,
            other => {
                return Err(format!(
                    "unknown loop subtype {other:?}: use for, foreach, while, dowhile, or infinite"
                ));
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
        _ => Err(format!(
            "unknown kind {s:?}: expected function_definition, if, loop, or loop:<subtype>"
        )),
    }
}

fn format_kind(k: UnifiedKind) -> String {
    match k {
        UnifiedKind::FunctionDefinition => "FunctionDefinition".to_string(),
        UnifiedKind::If => "If".to_string(),
        UnifiedKind::Loop(lk) => format!("Loop({lk:?})"),
    }
}

/// 1-based line, 0-based UTF-8 column within that line (byte offset from line start).
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

fn print_match_line(
    path: &Path,
    kind: &str,
    sl: usize,
    sc: usize,
    el: usize,
    ec: usize,
    name: Option<&str>,
    node_text: String,
    print_fields: Option<&[PrintField]>,
    print_format: Option<&str>,
) {
    let range = format!("{sl}:{sc}-{el}:{ec}");
    let start = format!("{sl}:{sc}");
    let end = format!("{el}:{ec}");
    let escaped_content = node_text.escape_default().to_string();
    let file = path.display().to_string();

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
            }
        }
        println!("{}", cols.join("\t"));
        return;
    }

    // Default output keeps the earlier shape for backward compatibility.
    if let Some(name) = name {
        println!(
            "{}\t{}\t{}\t{}",
            path.display(),
            kind,
            range,
            name
        );
    } else {
        println!("{}\t{}\t{}", path.display(), kind, range);
    }
}

fn render_template(
    template: &str,
    file: &str,
    kind: &str,
    start: &str,
    end: &str,
    range: &str,
    name: &str,
    content: &str,
) -> String {
    template
        .replace("{file}", file)
        .replace("{type}", kind)
        .replace("{start}", start)
        .replace("{end}", end)
        .replace("{range}", range)
        .replace("{name}", name)
        .replace("{content}", content)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintField {
    File,
    Type,
    Start,
    End,
    Content,
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
            _ => {
                return Err(format!(
                    "unknown field {token:?}; expected one of file,type,start,end,content or all"
                ));
            }
        };
        fields.push(field);
    }
    Ok(fields)
}
