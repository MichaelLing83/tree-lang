#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COVERAGE_DIR="${ROOT_DIR}/coverage"

cd "${ROOT_DIR}"

if [[ "${1:-}" == "--test-only" ]]; then
  echo "Running tests only..."
  cargo test --workspace
  exit 0
fi

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  cat <<'EOF'
error: cargo-llvm-cov is not installed.

Install it first:
  cargo install cargo-llvm-cov

Then run:
  ./test.sh
EOF
  exit 1
fi

echo "Running tests with coverage..."
mkdir -p "${COVERAGE_DIR}"

# Clean old llvm-cov artifacts to avoid stale stats.
cargo llvm-cov clean --workspace

# Run workspace tests and collect coverage data.
cargo llvm-cov --workspace --no-report

# Print a compact terminal summary (crate, file, line coverage %, total lines).
cargo llvm-cov report --json --summary-only 2>/dev/null \
  | python3 "${ROOT_DIR}/scripts/print_coverage_summary.py"

# Export HTML and LCOV reports.
cargo llvm-cov report --html --output-dir "${COVERAGE_DIR}"
cargo llvm-cov report --lcov --output-path "${COVERAGE_DIR}/lcov.info"

echo
echo "Coverage artifacts:"
echo "  HTML : ${COVERAGE_DIR}/html/index.html"
echo "  LCOV : ${COVERAGE_DIR}/lcov.info"
