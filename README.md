# tree-lang

`tree-lang` is a Rust-based source code analysis toolkit built on top of tree-sitter.
It provides a unified syntax model across multiple languages, so the same analysis flow can find constructs like `FunctionDefinition`, `Loop`, and `Branch` (if / switch / match) in different language grammars.

## Supported Languages (Current MVP)

- C
- C++
- Rust
- Python
- Java

## Unified kinds (names you pass vs names you see)

The same logical construct uses **different spellings** depending on whether you are typing a CLI flag or reading default output / `{type}` in `--print-format`.

| Layer | Role | Examples |
| ----- | ---- | -------- |
| **CLI input** | `-k` / `--kind`, and the `KIND` argument in `x.has(KIND)` / `x.is(KIND)` in `--step` | `function_definition`, `branch`, `branch:if`, `branch:switch`, `branch:match`, `loop`, `loop:for`, … |
| **CLI output** | Default `find` / traverse lines and `{type}` in templates | `FunctionDefinition`, `Branch(If)`, `Branch(Switch)`, `Loop(For)`, … |
| **Rust API** | `tree_lang_core::UnifiedKind`, `LoopKind`, `BranchKind` | e.g. `UnifiedKind::Branch(BranchKind::If)` |

Input is **ASCII**, case-insensitive, with `-` and `_` treated the same (`loop-for` ≡ `loop_for`).

**Loop subtypes (input):** use either `loop:<subtype>` or the same subtype inside parentheses to mirror output, for example:

- `loop:for` or **`loop(for)`** → `Loop(For)`
- `loop:foreach` or **`loop(foreach)`** / `loop(for_each)` → `Loop(ForEach)`
- `loop:while` or **`loop(while)`** → `Loop(While)`
- `loop:dowhile` or **`loop(dowhile)`** / `loop(do_while)` → `Loop(DoWhile)`
- `loop:infinite` or **`loop(infinite)`** / `loop(forever)` → `Loop(Infinite)` (Rust `loop { }`)

`-k loop` (no subtype) matches **any** of the above loop subtypes in one search.

**Branch subtypes (input):** same pattern as loops — `branch:<subtype>` or `branch(<subtype>)` mirroring `{type}` output:

- `branch:if` or **`branch(if)`** → `Branch(If)`
- `branch:switch` or **`branch(switch)`** → `Branch(Switch)` (C/C++/Java `switch`)
- `branch:match` or **`branch(match)`** → `Branch(Match)` (Rust `match`, Python `match`)

`-k branch` matches **any** of the three branch subtypes in one search.

### What counts as a `loop` (by language)

Classification is implemented in `crates/tree-lang/src/classify.rs` (tree-sitter **node type** string → unified kind). Only these nodes become `UnifiedKind::Loop(…)` today:

| Language | Tree-sitter node types treated as loops | Unified subtype |
| -------- | ---------------------------------------- | ---------------- |
| **Python** | `for_statement`, `while_statement` | `For`, `While` |
| **Java** | `for_statement`, `enhanced_for_statement`, `while_statement`, `do_statement` | `For`, `ForEach`, `While`, `DoWhile` |
| **Rust** | `for_expression`, `while_expression`, `loop_expression` | `For`, `While`, `Infinite` |
| **C** | `for_statement`, `while_statement`, `do_statement` | `For`, `While`, `DoWhile` |
| **C++** | `for_statement`, `for_range_loop`, `while_statement`, `do_statement` | `For`, `ForEach` (range-for), `While`, `DoWhile` |

Not modeled as `loop`: comprehensions / generator expressions as expressions-only constructs, macro-expanded bodies (tree-sitter sees the macro call, not the expanded loop), etc. If a grammar adds a new loop-shaped node, add it in `classify.rs`.

### What counts as a `branch` (by language)

| Language | Tree-sitter node types | Unified subtype |
| -------- | ---------------------- | ---------------- |
| **Python** | `if_statement` → If; `match_statement` → Match | `If`, `Match` |
| **Java** | `if_statement` → If; `switch_statement` → Switch | `If`, `Switch` |
| **Rust** | `if_expression` → If; `match_expression` → Match | `If`, `Match` |
| **C / C++** | `if_statement` → If; `switch_statement` → Switch | `If`, `Switch` |

### `function_definition`

| Unified kind | Typical tree-sitter roots (per language) |
| ------------ | ------------------------------------------ |
| `function_definition` | Rust `function_item`, Python `function_definition`, Java `method_declaration`, C/C++ `function_definition` |

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
  - target unified syntax kind (see **Unified kinds** above for naming and language mapping)
  - accepted values:
    - `function_definition` (also `fn`, `func`)
    - `branch` (all branch subtypes: if, switch, match)
    - `branch:if`, `branch:switch`, `branch:match` (also `branch(if)`, etc.)
    - `loop` (all loop subtypes)
    - `loop:for` or `loop(for)` (and similarly for other subtypes; see README table)
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
  - supported fields: `file`, `type`, `start`, `end`, `content`, `body`, `language`, `start_byte`, `end_byte`, `body_start_byte`, `body_end_byte`
  - `all` expands to all of the above in a fixed order
  - `content` / `body` are escaped (`\n`, `\t`, etc.) so each match stays on one logical output row when tab-separated

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
    - `{body}`, `{language}`, `{start_byte}`, `{end_byte}`, `{body_start_byte}`, `{body_end_byte}`

- `--step <STEP>` (repeatable; `find` only when not using function-definition name/param filters)
  - one statement per flag; order matters. Full grammar is in [**Step pipeline (detailed)**](#step-pipeline) below, and in `tree-lang find --help`.

<a id="step-pipeline"></a>
#### Step pipeline (`--step`)

Each `find` hit (or each unified visit in **Traverse** commands) can run a short **node-centric** pipeline. Steps share a set of **bindings**—named nodes plus the built-in names `node` and `current`—and execute in order. Unless you use `emit:`, a successful pipeline ends with the usual one-line output from `--print` / `--print-format`, using the **final** `current` and all bindings when expanding templates (see *Printing and templates* in this section).

**Bind names.** Use identifiers such as `x`, `b`, or `L2`. The right-hand side of an assignment is either a reserved word (`node`, `current`, `body`, `consequence`), a dotted field form `other.body` / `other.consequence`, or the query form `other.first(...)` / `other.body.first(...)` where `other` is an existing binding (not a free expression).

| Form | Meaning |
| ---- | ------- |
| `name=node` | Binds the **per-hit root**: the `find` / traverse match. This value does **not** change when the pipeline later moves `current` (it is not “whatever `current` is right now”). |
| `name=current` | Binds the **pipeline focus** at this step: whatever `current` is after any previous `is` / `has` / `first` (initially the same as `node`). |
| `name=body` | Same as `name=current.body` in spirit: the **primary body span** of the **current** node at this step. If the tree at that exact span is not a unified root, the implementation walks **into** the span in parse-tree order and takes the first inner construct that has a unified kind, while still using the **full** body span for operations that need the whole range (e.g. `x.has(…)`). If nothing fits, the whole hit is **skipped** (no error line for that candidate). |
| `name=consequence` | Same for the then-branch of **`branch:if` only** (equivalent to `=current.consequence` when it applies). If the current node is not an `if`, the hit is skipped. |
| `name=x.body` | The body span of an **existing** binding `x` (e.g. `b1=n.body` after `n=node`). Unknown `x` is a pipeline error. |
| `name=x.consequence` | The if-then span of binding `x` when it is a `branch:if` (same rules as `=consequence` on `current`). |
| `x.is(KIND)` | `KIND` uses the **same grammar as `-k`** (see *Unified kinds*). The binding `x` must be one of those kinds; on success, sets **`current` ← `x`**. If it fails, this match is dropped. |
| `x.has(KIND)` | Re-parses the text inside **`x`’s span** (as a sub-slice) and looks for a unified hit of the given `KIND` that is **strictly inside** the slice (the full-span match is not counted—same as “first inner” semantics). On success, sets **`current`** to that inner node. If there is no such node, the match is dropped. |
| `x.first(A, B, …)` | Each argument is a `KIND` list in `-k` syntax; the set of allowed `UnifiedKind` values is the **union** of all arguments. Walks the file’s parse tree **depth-first, preorder** starting from the node covering `x`’s span, **including** `x` if it already matches, and sets **`current`** to the first matching node. If none, the match is dropped. Comma is split at the top level only; nested `loop(for)`-style kind strings are written inside the parentheses, not as extra comma arguments. |
| `name=x.first(A, B, …)` | Same search as `x.first(...)`, but also stores the found node into `name`. The receiver may also be `x.body` or `x.consequence`, e.g. `name=x.body.first(loop)`. On success, it also sets **`current`** to that found node. |
| `emit:`*`TEMPLATE`* | Evaluates *TEMPLATE* like global `--print-format` and prints **one** line to stdout (mid-pipeline). Supports **dotted** placeholders `{` *name* `.` *field* `}` for any in-scope binding, plus the legacy placeholders (see the next subsection). If you use at least one `emit:`, the usual final one-line print for that hit is **not** produced (use `emit:` and/or design the pipeline so the last step is enough). |

**`node` and `current` are always in scope** as binding names, alongside any names you introduce with `=…`. They are updated on each step as described above (especially `current` after `is` / `has` / `first`).

**Printing and templates (after the last step, or in `emit:`).**

- **Dotted** placeholders: `{` *binding* `.` *field* `}`. *binding* is an identifier or `node` / `current`. *field* includes: `type`, `start`, `end`, `range`, `content`, `body`, `file`, `language`, `start_byte`, `end_byte`, `body_start` / `body_start_byte`, `body_end` / `body_end_byte` (and similar aliases as implemented in the tool).
- **Legacy** (single-name) placeholders such as `{type}`, `{file}`, `{range}` refer to the **final** `current` node, after all steps.
- `{node}` and `{current}` expand to the **escaped** full source text of the root hit and the final `current`, respectively.
- For any other binding *name* you created, `{`*name*`}` is replaced by that node’s full-span escaped text.
- Dotted and legacy can be combined in one template; a second expansion phase applies the legacy set after the dotted pass.

**`find` restriction:** `--step` cannot be combined with function-only filters on `find` (`--name`, `--param-*`, `--return-type`, `--param-name-at`, `--param-type-at`).

**Examples**

```bash
# Inner loop under an outer (body → has) and print the inner’s kind; same idea as a two-level “nested loop” check
tree-lang find ./src -l rust -k loop --step 'b=body' --step 'b.has(loop)' --print-format '{b.type} {file} {current.range}'

# Bind the hit, require it to be a loop, emit one line mid-pipeline
tree-lang find . -l c -k loop --step 'n=node' --step 'n.is(loop)' --step 'emit:found {n.file} {n.type}'

# Traverse: only unified visits; at each, filter with is(…) and custom format
tree-lang dfs_preorder ./src -l rust --step 'c=node' --step 'c.is(loop)' --print-format '{file} {c.range}'

# Bind the first function / branch / loop inside a body, then print fields from the binding
tree-lang dfs_preorder crates -l rust --step 'n=node' --step 'n.is(loop)' --step 'c1=n.body.first(function_definition, branch, loop)' --print-format '{c1.type} {c1.range}'
```

### Traverse commands (`dfs_*`, `bfs_*`)

Walk the **full** parse tree (every tree-sitter node in the chosen order). Whenever a node classifies as a unified kind (`function_definition`, any `branch`, or any `loop`), run the same `--print` / `--print-format` / `--step` pipeline from that node as `current`.

Subcommands:

- `dfs_preorder` — depth-first, preorder (same visit order family as `find`’s walk)
- `dfs_postorder` — depth-first, postorder
- `bfs_ltr` / `bfs_rtl` — breadth-first, left-to-right or right-to-left among siblings per level

They take paths, `-l` / `--language`, `-e` / `--exclude`, and the same printing / `--step` options as documented for `find` (without `-k` / function filters). Example:

```bash
tree-lang dfs_preorder ./src -l rust --step 'n=node' --step 'n.is(loop)' --step 'emit:{file} {type}'
```

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
    kind="branch:if",
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
