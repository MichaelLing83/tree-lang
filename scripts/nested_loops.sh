#!/usr/bin/env bash
# Demo: use traversal + pipeline to locate "directly nested loops".
#
# Definition used here:
# - Outer node must be a loop.
# - Enter its `body`.
# - Skip leading non-block syntax (plain statements / punctuation) via `strip`.
# - The first block-shaped unified node after stripping must be another loop.
#
# This models: outer loop body contains one or more ordinary statements, then a loop.
# (Current step DSL cannot strictly enforce "at least one" ordinary statement before
# the inner loop; this demo checks that the first block-shaped construct is a loop.)
#
# Pipeline explained:
#   assign:n:node  -> bind current span to n
#   is:n:loop      -> current root must be a loop (filter non-loops)
#   expand:b       -> move current into outer loop body span
#   strip          -> narrow current to first block-shaped node in that body
#   assign:s:node  -> bind stripped span
#   is:s:loop      -> stripped node must be a loop (direct nested loop hit)
#
# Options:
#   -m, --multiline  After each match, print file/range on separate lines, then the
#                    exact source bytes of the node (newlines preserved). Default is
#                    one TSV line per match; {content} in that mode is escaped, so
#                    newlines appear as \n in a single line.
#
# Usage:
#   scripts/nested_loops.sh [path] [language]
#   scripts/nested_loops.sh -m [path] [language]
#   TREE_LANG=./target/debug/tree-lang scripts/nested_loops.sh ./crates rust
#
# Output: one line per matched *outer* loop (default), or a multi-line block per
#         match with -m.

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
      cat <<'EOF'
Usage: nested_loops.sh [-m|--multiline] [path] [language]

  -m, --multiline   After each match, print file, range, then the raw source text
                    (newlines preserved). Default: one TSV line; {content} is escaped.
  -h, --help        Show this help.

Examples:
  scripts/nested_loops.sh ./src rust
  scripts/nested_loops.sh -m ./crates auto
EOF
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

echo "demo: direct nested loop detector (dfs_preorder + --step)"
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
    --step 'expand:b' \
    --step 'strip' \
    --step 'assign:s:node' \
    --step 'is:s:loop' \
    "$@" 2>/dev/null
}

if [[ "$MULTILINE" -eq 0 ]]; then
  run_treelang --print-format $'{file}\t{range}\t{content}'
else
  # Byte-accurate slice: preserves newlines in the terminal (unlike {content}).
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
