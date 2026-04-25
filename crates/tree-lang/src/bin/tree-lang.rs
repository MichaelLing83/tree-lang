use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use regex::Regex;
use walkdir::WalkDir;

use tree_lang::{
    find_function_definitions, find_unified_kinds, format_kind, for_each_subtree_node,
    kinds_from_cli, map_unified_node, parse, Language, MappedNode, TreeTraversal,
    UnifiedKind,
};

#[path = "../internal/pipeline.rs"]
mod pipeline;

/// Short help for `-h` (full step grammar is in [`STEP_ARG_LONG_HELP`]).
const STEP_ARG_HELP_SHORT: &str =
    "Pipeline step; repeat with --step (order matters). Use --help for full syntax.";

/// Long help for `--step` (shared by `find` and all traverse subcommands).
const STEP_ARG_LONG_HELP: &str = r"Pipeline steps — repeat `--step`; order matters (node-centric).

  name=node | current | body | consequence | x.body | x.consequence
      `=node` is the per-hit find/traverse root; `=current` is the pipeline focus. `=body` / `=consequence`
      are relative to **current**; `=x.body` / `=x.consequence` use another binding `x` (e.g. `b=n.body`).
      Does not move `current` by itself; later steps use the binding.

  x.is(KIND)          KIND uses the same grammar as -k. Fails the pipeline if `x` is
      not one of those kinds. Sets `current` to `x` on success.

  x.has(KIND)         Like the old has: first strict inner unified hit in `x`'s span
      (skips the full-span root). Sets `current` to that node.

  x.first(A,B,…)     Union of -k kind lists; first DFS-preorder node under `x` (including
      `x` if it matches) whose kind is in that set. Sets `current` on success.

  name=x.first(A,B,…) Same as x.first(...), but also binds the result to `name`.
      The receiver may also be x.body or x.consequence, e.g. c=x.body.first(loop).

  emit:TEMPLATE        One line of output mid-pipeline; `TEMPLATE` is like `--print-format`
      (dotted {x.y} for bindings plus legacy {type} etc. based on `current`).

`node` and `current` are always in scope. `--print` / `--print-format` after all steps
use the final `current` (dotted `name.field` bindings from after-pipeline state).

Restriction (subcommand find only): --step cannot be used with function_definition
filters (--name, --param-*, --return-type, --param-name-at, --param-type-at).

Example:
  --step 'n=node' --step 'b=n.body' --step 'b.has(loop)' --step 'emit:{b.type}'";

#[derive(Parser)]
#[command(name = "tree-lang", version, about = "Unified syntax search over source trees")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Shared flags for `dfs_preorder` / `dfs_postorder` / `bfs_ltr` / `bfs_rtl` (file selection and pipeline).
#[derive(Args)]
struct TraverseArgs {
    /// One or more files or directories to read.
    paths: Vec<PathBuf>,
    /// Exclude paths whose string form matches this regular expression (repeatable).
    #[arg(short = 'e', long = "exclude", value_name = "REGEX")]
    exclude: Vec<String>,
    /// Target language: c, cpp, java, python, rust, auto (default: auto).
    #[arg(short = 'l', long = "language", value_name = "LANG", default_value = "auto")]
    language: String,
    /// Print selected output fields: all or comma-separated fields (same as `find`).
    #[arg(
        long = "print",
        value_name = "FIELDS",
        num_args = 0..=1,
        default_missing_value = "all"
    )]
    print: Option<String>,
    /// Print using a format template (same as `find`).
    #[arg(long = "print-format", value_name = "TEMPLATE")]
    print_format: Option<String>,
    #[arg(
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
    /// Find unified syntax constructs in source files.
    Find(FindArgs),
    /// Walk the full parse tree depth-first, preorder (node then each child, left to right),
    /// running `--step` (if any) from each node that is a unified construct, in that order.
    #[command(name = "dfs_preorder")]
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

#[derive(Args)]
struct FindArgs {
    /// One or more files or directories to search.
    paths: Vec<PathBuf>,
    /// Exclude paths whose string form matches this regular expression (repeatable).
    #[arg(short = 'e', long = "exclude", value_name = "REGEX")]
    exclude: Vec<String>,
    /// Target language: c, cpp, java, python, rust, auto (default: auto).
    #[arg(short = 'l', long = "language", value_name = "LANG", default_value = "auto")]
    language: String,
    /// Unified syntax kind: function_definition, branch, branch:<subtype> (e.g. branch:if),
    /// branch(<subtype>) (e.g. branch(if)), loop, loop:<subtype>, loop(<subtype>).
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
    /// Fields: file,type,start,end,content,body,language,start_byte,end_byte,body_start_byte,body_end_byte
    /// `--print` (without value) is equivalent to `--print all`.
    #[arg(
        long = "print",
        value_name = "FIELDS",
        num_args = 0..=1,
        default_missing_value = "all"
    )]
    print: Option<String>,
    /// Print using a format template.
    /// Supported placeholders: {file},{type},{start},{end},{range},{name},{content},{body},{language},{start_byte},{end_byte},{body_start_byte},{body_end_byte}
    #[arg(long = "print-format", value_name = "TEMPLATE")]
    print_format: Option<String>,
    #[arg(
        long = "step",
        value_name = "STEP",
        action = clap::ArgAction::Append,
        help = STEP_ARG_HELP_SHORT,
        long_help = STEP_ARG_LONG_HELP
    )]
    step: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
enum LanguageSelector {
    Explicit(Language),
    Auto,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Find(args) => run_find(args),
        Command::DfsPreorder(a) => run_traverse(a, TreeTraversal::DfsPreorder),
        Command::DfsPostorder(a) => run_traverse(a, TreeTraversal::DfsPostorder),
        Command::BfsLtr(a) => run_traverse(a, TreeTraversal::BfsLtr),
        Command::BfsRtl(a) => run_traverse(a, TreeTraversal::BfsRtl),
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
    if program.is_some() && uses_function_filters {
        eprintln!("error: --step pipeline is not supported with function_definition filters (yet)");
        return std::process::ExitCode::from(2);
    }

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
            eprintln!("error: --language auto cannot be used with stdin; pass an explicit language");
            return std::process::ExitCode::from(2);
        }
        let mut source = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("<stdin>: read error: {e}");
            return std::process::ExitCode::from(1);
        }
        let language = match language_selector {
            LanguageSelector::Explicit(l) => l,
            LanguageSelector::Auto => unreachable!("guarded above"),
        };
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
            eprintln!("error: --language auto cannot be used with stdin; pass an explicit language");
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
                match pipeline::run_unified_pipeline(path, source, language, &m, prog) {
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
    pipeline: Option<&pipeline::StepProgram>,
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
            let escaped_body = f
                .body
                .map(|b| {
                    span_text(source, b.start_byte, b.end_byte)
                        .escape_default()
                        .to_string()
                })
                .unwrap_or_default();
            let body_start_byte = f
                .body
                .map(|b| b.start_byte.to_string())
                .unwrap_or_default();
            let body_end_byte = f.body.map(|b| b.end_byte.to_string()).unwrap_or_default();
            print_match_line(
                path,
                language,
                &format_kind(UnifiedKind::FunctionDefinition),
                sl,
                sc,
                el,
                ec,
                f.span.start_byte,
                f.span.end_byte,
                Some(&f.name),
                span_text(source, f.span.start_byte, f.span.end_byte),
                &escaped_body,
                &body_start_byte,
                &body_end_byte,
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
        if let Some(prog) = pipeline {
            for m in &matches {
                match pipeline::run_unified_pipeline(path, source, language, m, prog) {
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
            }
        } else {
            for m in matches {
                print_unified(path, source, language, &m, print_fields, print_format);
            }
        }
    }
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
    let body_start_byte = m
        .body
        .map(|b| b.start_byte.to_string())
        .unwrap_or_default();
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
    print_unified(
        path,
        source,
        language,
        &ap.current,
        print_fields,
        None,
    );
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
