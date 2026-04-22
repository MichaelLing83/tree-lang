#!/usr/bin/env bash
# For each loop found under the search root, look for other loops *inside that
# loop's body* (per tree-sitter's `body` field). If at least one inner loop
# exists, print the *outer* loop (full node text in bytes). Otherwise skip.
#
# Implementation: extract the body bytes to a temp file and run
# `tree-lang find … -k loop` on that slice with the same language.
#
# Requires: bash 4+, tree-lang on PATH, dd(1). No Python.
# CLI must support --print-format {body_start_byte} and {body_end_byte}.
#
# Usage:
#   scripts/nested_loops.sh [path]
#   TREE_LANG=path/to/tree-lang scripts/nested_loops.sh ./src
#
# The fmt= line below uses $'...' so the template has real tab characters.

set -e
set -o pipefail 2>/dev/null || true

ROOT=${1:-.}
T=${TREE_LANG:-tree-lang}

if ! command -v "$T" >/dev/null; then
  echo "error: tree-lang not found (set TREE_LANG=... if needed)" >&2
  exit 1
fi
if ! command -v dd >/dev/null; then
  echo "error: dd(1) is required for body extraction" >&2
  exit 1
fi

# One row per loop: body span, full node span, file, language, start.
fmt=$'{body_start_byte}\t{body_end_byte}\t{start_byte}\t{end_byte}\t{file}\t{language}\t{start}'

tmp=$(mktemp) || exit 1
"$T" find "$ROOT" -l auto -k loop 2>/dev/null --print-format "$fmt" >"$tmp" || true

any_line=0
valid=0
while IFS=$'\t' read -r bsb bbe sb se file lang st || [[ -n "${bsb:-}" ]]; do
  [[ -z "$bsb" && -z "$sb" && -z "$file" ]] && continue
  any_line=1

  if [[ "$bsb" == *"{"* ]]; then
    echo "error: tree-lang did not expand body byte placeholders; use a current build (e.g. cargo run -p tree-lang -- find …)" >&2
    rm -f "$tmp"
    exit 1
  fi
  if [[ ! "$bsb" =~ ^[0-9]+$ || ! "$bbe" =~ ^[0-9]+$ \
    || ! "$sb" =~ ^[0-9]+$ || ! "$se" =~ ^[0-9]+$ ]]; then
    continue
  fi
  valid=1
  ((bbe > bsb)) || continue

  blen=$((bbe - bsb))
  nlen=$((se - sb))
  if ((nlen <= 0)); then
    continue
  fi

  body_tmp=$(mktemp) || continue
  if ! dd if="$file" of="$body_tmp" bs=1 skip="$bsb" count="$blen" 2>/dev/null; then
    rm -f "$body_tmp"
    echo "warning: could not read body from $file, skipping" >&2
    continue
  fi

  inner_out=
  inner_out=$("$T" find "$body_tmp" -l "$lang" -k loop 2>/dev/null) || true
  rm -f "$body_tmp"

  if [[ -n "$inner_out" ]]; then
    printf '%s\n' "--- loop with inner loop in body ---"
    printf '%s\n' "file: $file"
    printf '%s\n' "start: $st"
    printf '%s\n' "content:"
    dd if="$file" bs=1 skip="$sb" count="$nlen" 2>/dev/null
    echo ""
    echo ""
  fi
done <"$tmp"
rm -f "$tmp"

if ((any_line == 1 && valid == 0)); then
  echo "error: no valid body/node byte offsets in tree-lang output" >&2
  exit 1
fi
exit 0
