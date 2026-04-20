#!/usr/bin/env python3
"""Read llvm-cov --json --summary-only on stdin; print a narrow table."""

import json
import sys


def crate_and_rel(path: str) -> tuple[str, str]:
    marker = "/crates/"
    i = path.find(marker)
    if i == -1:
        return "(unknown)", path.rsplit("/", 1)[-1]
    rest = path[i + len(marker) :]
    crate, sep, rel = rest.partition("/")
    return crate, rel or path.rsplit("/", 1)[-1]


def main() -> None:
    data = json.load(sys.stdin)
    entry = data["data"][0]
    files = entry["files"]
    totals = entry["totals"]["lines"]

    rows = []
    for f in files:
        path = f["filename"]
        lines = f["summary"]["lines"]
        crate, rel = crate_and_rel(path)
        rows.append((crate, rel, lines["percent"], lines["count"]))

    w_c = max((len(r[0]) for r in rows), default=0)
    w_f = max((len(r[1]) for r in rows), default=0)
    w_c = max(w_c, len("CRATE"))
    w_f = max(w_f, len("FILE"))

    print()
    print(f"{'CRATE'.ljust(w_c)}  {'FILE'.ljust(w_f)}  COVERAGE    LINES")
    print(f"{'-' * w_c}  {'-' * w_f}  --------  -----")
    for crate, rel, pct, n in rows:
        print(f"{crate.ljust(w_c)}  {rel.ljust(w_f)}  {pct:>6.1f}%  {n:>5}")
    print(
        f"{'TOTAL'.ljust(w_c)}  {''.ljust(w_f)}  {totals['percent']:>6.1f}%  {totals['count']:>5}"
    )
    print()


if __name__ == "__main__":
    main()
