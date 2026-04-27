#!/usr/bin/env bash
# End-to-end performance benchmarks for tree-lang CLI workflows.
#
# The script runs a small numbered suite with hyperfine, prints a per-run table,
# and records/updates a per-commit history table.
#
# Usage:
#   scripts/benchmark.sh
#   scripts/benchmark.sh --root crates --runs 10
#   scripts/benchmark.sh --case 2 --case 3
#
# History:
#   By default, results are stored in benchmarks/history.tsv. Re-running the
#   script on the same commit replaces that commit's rows for the selected cases.

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
cd "$REPO_ROOT"

ROOT=crates
TL_LANGUAGE=auto
TREE_LANG_BIN=${TREE_LANG:-"$REPO_ROOT/target/release/tree-lang"}
RUNS=${RUNS:-10}
WARMUP=${WARMUP:-1}
HISTORY_FILE=benchmarks/history.tsv
SELECTED_CASES=()
BUILD_RELEASE=1

usage() {
  cat <<'EOF'
Usage: benchmark.sh [OPTIONS]

Options:
  --root PATH         Source root to benchmark (default: crates)
  --lang LANG         tree-lang language selector (default: auto)
  --tree-lang PATH    tree-lang binary to run (default: $TREE_LANG or ./target/release/tree-lang)
  --runs N            hyperfine run count (default: 10, or $RUNS)
  --warmup N          hyperfine warmup count (default: 1, or $WARMUP)
  --history PATH      TSV history file (default: benchmarks/history.tsv)
  --case ID           Run only one numbered case; repeatable
  --no-build          Skip the default cargo build --release -p tree-lang step
  -h, --help          Show this help

Examples:
  scripts/benchmark.sh
  scripts/benchmark.sh --case 2 --case 3 --runs 5
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      ROOT=${2:?missing value for --root}
      shift 2
      ;;
    --lang)
      TL_LANGUAGE=${2:?missing value for --lang}
      shift 2
      ;;
    --tree-lang)
      TREE_LANG_BIN=${2:?missing value for --tree-lang}
      shift 2
      ;;
    --runs)
      RUNS=${2:?missing value for --runs}
      shift 2
      ;;
    --warmup)
      WARMUP=${2:?missing value for --warmup}
      shift 2
      ;;
    --history)
      HISTORY_FILE=${2:?missing value for --history}
      shift 2
      ;;
    --case)
      SELECTED_CASES+=("${2:?missing value for --case}")
      shift 2
      ;;
    --no-build)
      BUILD_RELEASE=0
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option: $1 (try --help)" >&2
      exit 2
      ;;
  esac
done

if ! command -v hyperfine >/dev/null; then
  echo "error: hyperfine not found. Install it first (for example: cargo install hyperfine)." >&2
  exit 1
fi

if ! command -v python3 >/dev/null; then
  echo "error: python3 is required to parse hyperfine JSON and render tables." >&2
  exit 1
fi

if [[ "$BUILD_RELEASE" -eq 1 ]]; then
  echo "building release binary: cargo build --release -p tree-lang"
  cargo build --release -p tree-lang
  echo ""
fi

if [[ "$TREE_LANG_BIN" == */* ]]; then
  if [[ ! -x "$TREE_LANG_BIN" ]]; then
    echo "error: tree-lang binary is not executable: $TREE_LANG_BIN" >&2
    echo "hint: build the current checkout first: cargo build --release -p tree-lang" >&2
    exit 1
  fi
elif ! command -v "$TREE_LANG_BIN" >/dev/null; then
  echo "error: tree-lang not found: $TREE_LANG_BIN (set TREE_LANG=... or pass --tree-lang)" >&2
  exit 1
fi

quote() {
  printf '%q' "$1"
}

T=$(quote "$TREE_LANG_BIN")
R=$(quote "$ROOT")
L=$(quote "$TL_LANGUAGE")

CASE_IDS=(
  "1"
  "2"
  "3"
  "4"
)

CASE_NAMES=(
  "find alias loop pipeline"
  "dfs_preorder triple loop pipeline"
  "find alias triple loop pipeline"
  "dfs_preorder unified baseline"
)

CASE_COMMANDS=(
  "$T find $R -l $L -s 'node.is(loop)' --print-format '{file}: {type}' >/dev/null 2>/dev/null"
  "$T dfs_preorder $R -l $L --step 'node.is(loop)' --step 'b1=node.body' --step 'c1=b1.first(function_definition, branch, loop)' --step 'c1.is(loop)' --step 'c2=c1.body.first(function_definition,branch,loop)' --step 'c2.is(loop)' --print-format '{node.file}: {node}' >/dev/null 2>/dev/null"
  "$T find $R -l $L -s 'node.is(loop)' -s 'b1=node.body' -s 'c1=b1.first(function_definition, branch, loop)' -s 'c1.is(loop)' -s 'c2=c1.body.first(function_definition,branch,loop)' -s 'c2.is(loop)' --print-format '{node.file}: {node}' >/dev/null 2>/dev/null"
  "$T dfs_preorder $R -l $L --print-format '{file}: {type}' >/dev/null 2>/dev/null"
)

case_selected() {
  local id=$1
  if [[ ${#SELECTED_CASES[@]} -eq 0 ]]; then
    return 0
  fi
  local selected
  for selected in "${SELECTED_CASES[@]}"; do
    if [[ "$selected" == "$id" ]]; then
      return 0
    fi
  done
  return 1
}

commit=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
current_tsv="$tmpdir/current.tsv"

printf 'commit\ttimestamp\tcase_id\tcase_name\tmean_s\tstddev_s\tmin_s\tmax_s\tuser_s\tsystem_s\tcommand\n' >"$current_tsv"

echo "tree-lang benchmark"
echo "commit:  $commit"
echo "root:    $ROOT"
echo "lang:    $TL_LANGUAGE"
echo "binary:  $TREE_LANG_BIN"
echo "runs:    $RUNS"
echo "warmup:  $WARMUP"
echo ""

for idx in "${!CASE_IDS[@]}"; do
  case_id=${CASE_IDS[$idx]}
  case_name=${CASE_NAMES[$idx]}
  case_cmd=${CASE_COMMANDS[$idx]}

  if ! case_selected "$case_id"; then
    continue
  fi

  json="$tmpdir/case-$case_id.json"
  echo "[$case_id] $case_name"
  hyperfine --warmup "$WARMUP" --runs "$RUNS" --export-json "$json" "$case_cmd"

  python3 - "$json" "$current_tsv" "$commit" "$timestamp" "$case_id" "$case_name" "$case_cmd" <<'PY'
import csv
import json
import sys

json_path, out_path, commit, timestamp, case_id, case_name, command = sys.argv[1:]
with open(json_path, "r", encoding="utf-8") as f:
    result = json.load(f)["results"][0]

def metric(name):
    value = result.get(name)
    return 0.0 if value is None else float(value)

row = {
    "commit": commit,
    "timestamp": timestamp,
    "case_id": case_id,
    "case_name": case_name,
    "mean_s": f"{metric('mean'):.6f}",
    "stddev_s": f"{metric('stddev'):.6f}",
    "min_s": f"{metric('min'):.6f}",
    "max_s": f"{metric('max'):.6f}",
    "user_s": f"{metric('user'):.6f}",
    "system_s": f"{metric('system'):.6f}",
    "command": command,
}
with open(out_path, "a", encoding="utf-8", newline="") as f:
    writer = csv.DictWriter(f, fieldnames=row.keys(), delimiter="\t")
    writer.writerow(row)
PY
  echo ""
done

ran_cases=$(($(wc -l <"$current_tsv") - 1))
if ((ran_cases == 0)); then
  echo "error: no benchmark cases selected" >&2
  exit 2
fi

history_dir=$(dirname "$HISTORY_FILE")
mkdir -p "$history_dir"

python3 - "$HISTORY_FILE" "$current_tsv" <<'PY'
import csv
import os
import sys

history_path, current_path = sys.argv[1:]
fieldnames = [
    "commit",
    "timestamp",
    "case_id",
    "case_name",
    "mean_s",
    "stddev_s",
    "min_s",
    "max_s",
    "user_s",
    "system_s",
    "command",
]

def read_rows(path):
    if not os.path.exists(path) or os.path.getsize(path) == 0:
        return []
    with open(path, "r", encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))

current = read_rows(current_path)
replace_keys = {(r["commit"], r["case_id"]) for r in current}
history = [r for r in read_rows(history_path) if (r["commit"], r["case_id"]) not in replace_keys]
rows = history + current

with open(history_path, "w", encoding="utf-8", newline="") as f:
    writer = csv.DictWriter(f, fieldnames=fieldnames, delimiter="\t")
    writer.writeheader()
    writer.writerows(rows)
PY

echo "Current Run"
python3 - "$current_tsv" <<'PY'
import csv
import sys

rows = list(csv.DictReader(open(sys.argv[1], encoding="utf-8"), delimiter="\t"))
print("| Case | Name | Mean (s) | Stddev (s) | Min (s) | Max (s) |")
print("| ---- | ---- | -------- | ---------- | ------- | ------- |")
for r in rows:
    print(
        f"| {r['case_id']} | {r['case_name']} | "
        f"{float(r['mean_s']):.3f} | {float(r['stddev_s']):.3f} | "
        f"{float(r['min_s']):.3f} | {float(r['max_s']):.3f} |"
    )
PY

echo ""
echo "History (mean seconds)"
python3 - "$HISTORY_FILE" <<'PY'
import csv
import sys
from collections import OrderedDict

rows = list(csv.DictReader(open(sys.argv[1], encoding="utf-8"), delimiter="\t"))
case_names = OrderedDict()
by_commit = OrderedDict()

for r in rows:
    case_names.setdefault(r["case_id"], r["case_name"])
    entry = by_commit.setdefault(r["commit"], {"timestamp": r["timestamp"], "cases": {}})
    entry["timestamp"] = r["timestamp"]
    entry["cases"][r["case_id"]] = r

case_ids = sorted(case_names, key=lambda x: int(x) if x.isdigit() else x)
headers = ["Commit", "Timestamp"] + [f"{cid}: {case_names[cid]}" for cid in case_ids]
print("| " + " | ".join(headers) + " |")
print("| " + " | ".join(["---"] * len(headers)) + " |")
for commit, entry in by_commit.items():
    vals = [commit, entry["timestamp"]]
    for cid in case_ids:
        r = entry["cases"].get(cid)
        vals.append(f"{float(r['mean_s']):.3f}" if r else "")
    print("| " + " | ".join(vals) + " |")
PY

echo ""
echo "history: $HISTORY_FILE"
