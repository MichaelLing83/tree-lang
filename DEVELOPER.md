# DEVELOPER Guide (MVP)

This document explains how the current MVP works and how to extend it safely.

## Scope

MVP supports these input languages:

- C
- C++
- Java
- Rust
- Python

MVP extracts these unified syntax constructs:

- `FunctionDefinition`
- `Loop(For | ForEach | While | DoWhile | Infinite)`
- `If`

## Crate Layout

- `crates/tree-lang-core`
  - Language-agnostic data model.
  - Defines `Span`, `LoopKind`, `UnifiedKind`, and `MappedNode`.
- `crates/tree-lang`
  - tree-sitter integration.
  - Exposes `Language`, `parse`, and `extract_unified`.
  - Contains per-language mapping logic in `src/classify.rs`.

## How It Works

### 1) Parse source with tree-sitter

Call:

- `parse(language, source) -> Result<Tree, ParseError>`

Implementation details:

- Creates a `tree_sitter::Parser`.
- Selects grammar by `Language::tree_sitter_language()`.
- Parses source and returns a syntax tree.

### 2) Walk the full syntax tree (DFS)

Call:

- `extract_unified(language, &tree) -> Vec<MappedNode>`

Implementation details:

- Recursively visits every node.
- Reads node kind string via `node.kind()`.
- Builds byte span from `node.range().start_byte/end_byte`.
- Delegates classification to `classify(language, kind, span)`.

### 3) Map grammar-specific node kinds to unified kinds

`src/classify.rs` contains the MVP mapping table.

Examples:

- Python:
  - `function_definition` -> `FunctionDefinition`
  - `for_statement` -> `Loop(For)`
  - `while_statement` -> `Loop(While)`
  - `if_statement` -> `Branch(If)`
  - `match_statement` -> `Branch(Match)`
- Rust:
  - `function_item` -> `FunctionDefinition`
  - `for_expression` -> `Loop(For)`
  - `while_expression` -> `Loop(While)`
  - `loop_expression` -> `Loop(Infinite)`
  - `if_expression` -> `Branch(If)`
  - `match_expression` -> `Branch(Match)`
- Java:
  - `method_declaration` -> `FunctionDefinition`
  - `enhanced_for_statement` -> `Loop(ForEach)`
  - plus `for_statement`, `while_statement`, `do_statement`, `if_statement` -> `Branch(If)`, `switch_statement` -> `Branch(Switch)`
- C / C++:
  - `function_definition` -> `FunctionDefinition`
  - C++ `for_range_loop` -> `Loop(ForEach)`
  - plus `for_statement`, `while_statement`, `do_statement`, `if_statement` -> `Branch(If)`, `switch_statement` -> `Branch(Switch)`

Any node kind not in the table returns `None` and is ignored by MVP extraction.

## Data Model Notes

- `Span` uses byte offsets in UTF-8 source text.
- `MappedNode` only stores:
  - unified kind
  - source span
- Current MVP intentionally avoids symbol resolution and type analysis.

## Testing

Integration tests are in `crates/tree-lang/tests/mvp.rs`.

Test strategy:

- One sample snippet per language.
- Assert exact extracted `UnifiedKind` sequence.
- Covers all MVP categories (function, loop variants, if).

Run:

```bash
cargo test
```

## How to Extend

When adding a new syntax concept or language:

1. Add/adjust core enum(s) in `tree-lang-core` only if concept is truly cross-language.
2. Add language-specific mapping in `crates/tree-lang/src/classify.rs`.
3. Add integration tests for each affected language.
4. Keep MVP behavior stable (avoid breaking existing kind names/semantics).

Recommended rule:

- Prefer a small stable unified surface.
- Put language quirks in mapping logic, not in analysis call sites.

## Known MVP Limits

- No AST normalization beyond direct node-kind mapping.
- No symbol table, scope tracking, or control-flow graph.
- No incremental parsing API yet.
- No CLI/binary yet (library-only).
