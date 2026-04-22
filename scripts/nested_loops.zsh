#!/usr/bin/env zsh
# List "outer" loops in a nest: a loop that (1) has at least one other loop
# strictly inside its [start_byte, end_byte) span on disk, and (2) is not
# itself strictly inside any other loop's span.
#
# For each such loop, print: file, start (line:col), then full content (unescaped)
# of the loop node, separated with blank lines and a --- marker between records.
#
# Requires: tree-lang (this repo) on PATH, and python3.
#
# Usage:
#   scripts/nested_loops.zsh [path]
#   TREE_LANG=path/to/tree-lang scripts/nested_loops.zsh ./src
#
# Note: the first find step assumes paths printed by tree-lang do not contain tab
# characters. Lines are split on the first 4 tabs: start_byte, end_byte, file, rest is language\\t{start}...
# Actually we use exactly 5 tab-separated fields: sb, eb, file, language, start

emulate -L zsh
set -o err_exit
set -o pipefail 2>/dev/null || true

local ROOT=${1:-.}
local T=${TREE_LANG:-tree-lang}

if ! command -v "$T" >/dev/null; then
  print -u2 "error: tree-lang not found (set TREE_LANG=... if needed)"
  exit 1
fi
if ! command -v python3 >/dev/null; then
  print -u2 "error: python3 is required"
  exit 1
fi

# One line per loop: UTF-8 byte span + path + language + {start} (line:col).
# Use $'...' so \t is a real tab (single-quoted '\t' is backslash + t, not tab).
"$T" find "$ROOT" -l auto -k loop 2>/dev/null --print-format $'{start_byte}\t{end_byte}\t{file}\t{language}\t{start}' | python3 -c '
import json
import sys

def load_loops():
    out = []
    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line.strip():
            continue
        # start_byte, end_byte, file, language, start — file may contain no TAB
        parts = line.split("\t", 4)
        if len(parts) < 5:
            continue
        sb, eb, file, language, start = parts
        try:
            sb_i, eb_i = int(sb), int(eb)
        except ValueError:
            continue
        out.append(
            {
                "start_byte": sb_i,
                "end_byte": eb_i,
                "file": file,
                "language": language,
                "start": start,
            }
        )
    return out

def strictly_inside(outer, inner) -> bool:
    if inner["file"] != outer["file"]:
        return False
    a0, a1 = outer["start_byte"], outer["end_byte"]
    b0, b1 = inner["start_byte"], inner["end_byte"]
    return a0 < b0 and a1 > b1

def read_span_text(path: str, sb: int, eb: int) -> str:
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        data = f.read()
    if sb < 0 or eb > len(data) or sb > eb:
        return ""
    return data[sb:eb]

def main():
    loops = load_loops()
    n = len(loops)
    has_inner = [False] * n
    contained_by = [False] * n
    for i in range(n):
        for j in range(n):
            if i == j:
                continue
            if strictly_inside(loops[i], loops[j]):
                has_inner[i] = True
            if strictly_inside(loops[j], loops[i]):
                contained_by[i] = True
    for i, L in enumerate(loops):
        if not has_inner[i]:
            continue
        if contained_by[i]:
            continue
        text = read_span_text(L["file"], L["start_byte"], L["end_byte"])
        rec = {
            "file": L["file"],
            "start": L["start"],
            "content": text,
        }
        print(json.dumps(rec, ensure_ascii=False))
        print()

if __name__ == "__main__":
    main()
'
