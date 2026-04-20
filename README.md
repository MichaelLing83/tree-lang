# tree-lang

`tree-lang` is a Rust-based source code analysis toolkit built on top of tree-sitter.
It provides a unified syntax model across multiple languages, so the same analysis flow can find constructs like `FunctionDefinition`, `Loop`, and `If` in different language grammars.

## Supported Languages (Current MVP)

- C
- C++
- Rust
- Python
- Java

## CLI

The project provides a command-line tool also named `tree-lang`.

### `find` command

Find unified syntax nodes in one or more files/directories.

```bash
tree-lang find <PATH> [<PATH> ...] --language <LANG> --kind <KIND> [--exclude <REGEX> ...]
```

#### Arguments

- positional `PATH ...`
  - one or more file or directory paths
  - if a file is given, it is analyzed directly
  - if a directory is given, `tree-lang` recursively scans files matching the selected language extensions

#### Options

- `-l, --language <LANG>`
  - target language
  - accepted values: `c`, `cpp` (also `c++`, `cxx`), `java`, `python` (also `py`), `rust` (also `rs`)

- `-k, --kind <KIND>`
  - target unified syntax kind
  - accepted values:
    - `function_definition` (also `fn`, `func`)
    - `if`
    - `loop` (all loop subtypes)
    - `loop:for`
    - `loop:foreach`
    - `loop:while`
    - `loop:dowhile`
    - `loop:infinite`

- `-e, --exclude <REGEX>`
  - exclude file paths by regular expression (Rust regex syntax)
  - repeatable; if any regex matches the path string, the file is skipped

- `-n, --name <REGEX>`
  - filter found structure names by regular expression
  - currently supported when `--kind function_definition`
  - useful for matching function names (for example `^parse_` or `.*init.*`)

### Output Format

Each match is printed as one line:

```text
<file_path>\t<UnifiedKind>\t<start_line>:<start_col>-<end_line>:<end_col>
```

Example:

```text
crates/tree-lang/tests/mvp.rs	FunctionDefinition	12:0-34:1
```

## Build / Release CLI Binary

Use the provided script:

```bash
./build_and_release.sh
```

This will:

1. compile release binary `tree-lang`
2. install/copy it to `~/.cargo/bin/tree-lang`

After that, ensure `~/.cargo/bin` is in your `PATH`, then run:

```bash
tree-lang --help
```
