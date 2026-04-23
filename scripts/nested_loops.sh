#!/usr/bin/env bash
# For each loop under the search root, if tree-sitter's `body` of that loop
# contains another loop (unified "loop" kind), print the *outer* loop: file,
# start (line:col), then the full loop text (raw bytes from the file).
#
# Uses a single `tree-lang find` with --step: bind the body span, then require
# at least one inner `loop` inside it. Same logic as a manual body slice + 2nd
# find, but in-process.
#
# Requires: bash 4+, tree-lang (with `find` --step), dd(1). No Python.
#
# Usage:
#   scripts/nested_loops.sh [path]
#   TREE_LANG=path/to/tree-lang scripts/nested_loops.sh ./src
#
# --print-format below uses $'...' for real tab characters.

set -e
set -o pipefail 2>/dev/null || true

ROOT=${1:-.}
T=${TREE_LANG:-tree-lang}

if ! command -v "$T" >/dev/null; then
  echo "error: tree-lang not found (set TREE_LANG=... if needed)" >&2
  exit 1
fi
if ! command -v dd >/dev/null; then
  echo "error: dd(1) is required for full loop text" >&2
  exit 1
fi

# One line per *outer* loop that has an inner loop in its body: byte span + location.
fmt=$'{start_byte}\t{end_byte}\t{file}\t{start}'

tmp=$(mktemp) || exit 1
# shellcheck disable=SC2016
if ! "$T" find "$ROOT" -l auto -k loop 2>/dev/null \
  --step 'assign:ob:body' \
  --step 'has:ob:loop' \
  --print-format "$fmt" >"$tmp"
then
  rm -f "$tmp"
  echo "error: tree-lang find failed (need a build with \`find --step\`)" >&2
  exit 1
fi

while IFS=$'\t' read -r sb se file st || [[ -n "${sb:-}" ]]; do
  [[ -z "$sb" && -z "$se" && -z "$file" ]] && continue

  if [[ "$sb" == *"{"* ]]; then
    echo "error: tree-lang did not expand placeholders; use a current build" >&2
    rm -f "$tmp"
    exit 1
  fi
  if [[ ! "$sb" =~ ^[0-9]+$ || ! "$se" =~ ^[0-9]+$ ]]; then
    continue
  fi

  len=$((se - sb))
  if ((len <= 0)); then
    continue
  fi

  printf '%s\n' "--- loop with inner loop in body ---"
  printf '%s\n' "file: $file"
  printf '%s\n' "start: $st"
  printf '%s\n' "content:"
  dd if="$file" bs=1 skip="$sb" count="$len" 2>/dev/null
  echo ""
  echo ""
done <"$tmp"
rm -f "$tmp"
exit 0
