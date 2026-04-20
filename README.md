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
  - use `-` to read source from stdin

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

- `-p, --param-name <REGEX>`
  - filter by function parameter name (matches any parameter)
  - repeatable; each provided regex must match at least one parameter name
  - currently supported when `--kind function_definition`

- `-t, --param-type <REGEX>`
  - filter by function parameter type (matches any parameter)
  - repeatable; each provided regex must match at least one parameter type
  - currently supported when `--kind function_definition`

- `--param-name-at <IDX:REGEX>`
  - filter by function parameter name at a specific position (0-indexed)
  - repeatable
  - example: `--param-name-at 0:^self$`
  - currently supported when `--kind function_definition`

- `--param-type-at <IDX:REGEX>`
  - filter by function parameter type at a specific position (0-indexed)
  - repeatable
  - example: `--param-type-at 2:^AttrWrapper$`
  - currently supported when `--kind function_definition`

- `--print [FIELDS]`
  - customize output fields for each match
  - `--print` without value is equivalent to `--print all`
  - supported fields: `file`, `type`, `start`, `end`, `content`
  - `all` expands to `file,type,start,end,content`
  - content is escaped (`\n`, `\t`, etc.) to keep one match per output line

- `--print-format <TEMPLATE>`
  - custom output template for each match
  - cannot be used together with `--print`
  - supported placeholders:
    - `{file}`
    - `{type}`
    - `{start}` (like `12:0`)
    - `{end}` (like `34:1`)
    - `{range}` (like `12:0-34:1`)
    - `{name}` (empty if not available)
    - `{content}` (escaped node text)

### Output Format

Each match is printed as one line:

```text
<file_path>\t<UnifiedKind>\t<start_line>:<start_col>-<end_line>:<end_col>
```

When function filters are used (`-n`, `-p`, `-t`, `--param-name-at`, `--param-type-at`), an additional function name column is included.

When `--print` is not set, output keeps the default format:

```text
<file_path>\t<UnifiedKind>\t<start_line>:<start_col>-<end_line>:<end_col>
```

When `--print` is set, output columns are exactly controlled by `FIELDS`.

Examples:

```bash
# Equivalent to --print all
tree-lang find src -l rust -k function_definition --print

# Custom field order / subset
tree-lang find src -l rust -k function_definition --print file,type,start,end
tree-lang find src -l rust -k function_definition --print type,content

# Custom format template
tree-lang find src -l rust -k function_definition \
  --print-format '{file}\t{type}\t{name}\t{range}\t{content}'
```

Example:

```text
crates/tree-lang/tests/mvp.rs	FunctionDefinition	12:0-34:1
```

With function name + parameter filters:

```bash
tree-lang find crates/tree-lang/tests/data/rust \
  -l rust \
  -k function_definition \
  -n '^parse_' \
  -p '^attrs$' \
  --param-type-at '1:^Bound$'

# Read source from stdin
cat file.rs | tree-lang find - -l rust -k function_definition
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

## Python Wheel (WHL)

The repository includes a PyO3 binding crate (`crates/tree-lang-py`) so the analyzer can be used directly from Python on major operating systems.

### Build locally

```bash
python -m pip install maturin
maturin build --release --manifest-path crates/tree-lang-py/Cargo.toml
```

Or install into the current virtualenv for development:

```bash
maturin develop --manifest-path crates/tree-lang-py/Cargo.toml
```

### Python API (first version)

```python
import tree_lang

matches = tree_lang.find_in_source(
    source="fn f(x: i32) { if x > 0 {} }",
    language="rust",
    kind="function_definition",
)

path_matches = tree_lang.find_in_paths(
    paths=["src"],
    language="rust",
    kind="if",
    exclude=[r"/target/"],
)
```

`find_in_source()` and `find_in_paths()` return a list of dictionaries with fields such as:

- `file` (for path mode)
- `kind`
- `start_byte`, `end_byte`
- `start_line`, `start_col`, `end_line`, `end_col`
- `content`

### CI wheel publishing

A GitHub Actions workflow is provided at `.github/workflows/python-wheels.yml`:

- Builds wheels on Linux/macOS/Windows
- Builds source distribution (`sdist`)
- Publishes to PyPI on `v*` tags (recommended with Trusted Publishing)
