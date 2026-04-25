#!/usr/bin/env bash
# Demo: use traversal + pipeline to locate "directly nested" three-level loops.
#
# Definition: from an outermost loop (L1), the first ``strict inner'' loop in L1's body
# is L2, and the first strict inner loop in L2's body is L3.
#
# Pipeline (each --step, in order):
#   n=node, n.is(loop)   -> L1
#   b1=n.body, b1.has(loop) -> L2, then l2=current, l2.is(loop)
#   b2=l2.body, b2.has(loop) -> L3, then l3=current, l3.is(loop)
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
    --step 'n=node' \
    --step 'n.is(loop)' \
    --step 'b1=n.body' \
    --step 'b1.has(loop)' \
    --step 'l2=current' \
    --step 'l2.is(loop)' \
    --step 'b2=l2.body' \
    --step 'b2.has(loop)' \
    --step 'l3=current' \
    --step 'l3.is(loop)' \
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
