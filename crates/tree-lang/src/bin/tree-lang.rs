use std::fs;
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
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Find(args) => run_find(args),
    }
}

fn run_find(args: FindArgs) -> std::process::ExitCode {
    if args.paths.is_empty() {
        eprintln!("error: at least one path is required");
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
    if name_regex.is_some() && !supports_name_filter(&kinds) {
        eprintln!("error: --name is currently supported only with --kind function_definition");
        return std::process::ExitCode::from(2);
    }

    let mut files = Vec::new();
    for root in &args.paths {
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
    for path in files {
        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: read error: {e}", path.display());
                had_error = true;
                continue;
            }
        };
        if let Some(name_re) = &name_regex {
            let functions = match find_function_definitions(language, &source) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}: parse error: {e}", path.display());
                    had_error = true;
                    continue;
                }
            };
            for f in functions.into_iter().filter(|f| name_re.is_match(&f.name)) {
                let (sl, sc) = byte_to_line_col(&source, f.span.start_byte);
                let (el, ec) = byte_to_line_col(&source, f.span.end_byte);
                println!(
                    "{}\t{}\t{}:{}-{}:{}\t{}",
                    path.display(),
                    format_kind(UnifiedKind::FunctionDefinition),
                    sl,
                    sc,
                    el,
                    ec,
                    f.name
                );
            }
        } else {
            let matches = match find_unified_kinds(language, &source, &kinds) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("{}: parse error: {e}", path.display());
                    had_error = true;
                    continue;
                }
            };
            for m in matches {
                let (sl, sc) = byte_to_line_col(&source, m.span.start_byte);
                let (el, ec) = byte_to_line_col(&source, m.span.end_byte);
                println!(
                    "{}\t{}\t{}:{}-{}:{}",
                    path.display(),
                    format_kind(m.kind),
                    sl,
                    sc,
                    el,
                    ec
                );
            }
        }
    }

    if had_error {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn supports_name_filter(kinds: &[UnifiedKind]) -> bool {
    kinds.len() == 1 && kinds[0] == UnifiedKind::FunctionDefinition
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
