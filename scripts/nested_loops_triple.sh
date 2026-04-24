#!/usr/bin/env bash
# Demo: use traversal + pipeline to locate "directly nested" three-level loops.
#
# Definition used here (same family as scripts/nested_loops.sh, one more level):
# - Outermost node must be a loop.
# - Enter its `body`, `strip` to the first block-shaped node -> must be a loop (L2).
# - Enter L2's `body`, `strip` to the first block-shaped node -> must be a loop (L3).
#
# Same limitation as the two-level script: this checks that after each strip, the
# first block-shaped unified node is a loop; it does not count "N plain statements
# then a loop" in the current DSL.
#
# Pipeline (each line is one --step, in order):
#   assign:n:node  -> bind current
#   is:n:loop      -> L1 must be a loop
#   expand:o       -> current := parse of L1 body (re-rooted)
#   strip          -> first block in L1 body
#   assign:m:node  -> bind
#   is:m:loop      -> L2 must be a loop
#   expand:mi      -> current := parse of L2 body
#   strip          -> first block in L2 body
#   assign:i:node  -> bind
#   is:i:loop      -> L3 must be a loop (three-level direct nesting under this model)
#
# Options:
#   -m, --multiline  For each match, print file, range, then the raw source span
#                    (newlines preserved). Default is one escaped TSV line; see
#                    scripts/nested_loops.sh for details.
#
# Usage:
#   scripts/nested_loops_triple.sh [path] [language]
#   scripts/nested_loops_triple.sh -m [path] [language]
#   TREE_LANG=./target/debug/tree-lang scripts/nested_loops_triple.sh ./crates rust
#
# Output: one line per matched *outermost* loop (L1) that satisfies the chain.
#
# Troubleshooting: if you only see the demo banner and no further lines, there were
# zero matches (this pattern is strict). tree-lang errors go to stderr — do not
# silence stderr when debugging. A tiny fixture that should match:
#   scripts/nested_loops_triple.sh crates/tree-lang/tests/data/rust/triple_nested_for.rs rust

set -e
set -o pipefail 2>/dev/null || true

MULTILINE=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -m | --multiline)
      MULTILINE=1
      shift
      ;;
    -h | --help)
      sed -n '2,/^$/p' "$0" | head -n 45
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "error: unknown option: $1 (try -h)" >&2
      exit 2
      ;;
    *) break ;;
  esac
done

ROOT=${1:-.}
LANG=${2:-auto}
T=${TREE_LANG:-tree-lang}

if ! command -v "$T" >/dev/null; then
  echo "error: tree-lang not found (set TREE_LANG=... if needed)" >&2
  exit 1
fi

if [[ "$MULTILINE" -eq 1 ]] && ! command -v dd >/dev/null; then
  echo "error: -m / --multiline requires dd(1)" >&2
  exit 1
fi

echo "demo: three-level direct nested loop detector (dfs_preorder + --step)"
echo "root: $ROOT"
echo "language: $LANG"
if [[ "$MULTILINE" -eq 1 ]]; then
  echo "output: multiline (file + range + raw source span)"
else
  echo "output: one TSV line per match ({content} is escaped; use -m for readable newlines)"
fi
echo ""

run_treelang() {
  "$T" dfs_preorder "$ROOT" -l "$LANG" \
    --step 'assign:n:node' \
    --step 'is:n:loop' \
    --step 'expand:o' \
    --step 'strip' \
    --step 'assign:m:node' \
    --step 'is:m:loop' \
    --step 'expand:mi' \
    --step 'strip' \
    --step 'assign:i:node' \
    --step 'is:i:loop' \
    "$@"
}

if [[ "$MULTILINE" -eq 0 ]]; then
  run_treelang --print-format $'{file}\t{range}\t{content}'
else
  run_treelang --print-format $'{file}\t{start_byte}\t{end_byte}\t{range}' | while IFS=$'\t' read -r f sb se range || [[ -n "${f:-}" ]]; do
    [[ -z "$f" ]] && continue
    if [[ ! "$sb" =~ ^[0-9]+$ || ! "$se" =~ ^[0-9]+$ ]]; then
      continue
    fi
    len=$((se - sb))
    if ((len <= 0)); then
      continue
    fi
    echo "---"
    echo "file:  $f"
    echo "range: $range"
    echo "content:"
    dd if="$f" bs=1 skip="$sb" count="$len" 2>/dev/null
    echo ""
  done
fi
