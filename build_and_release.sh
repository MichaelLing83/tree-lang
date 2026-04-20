#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="tree-lang"
DEST_DIR="${HOME}/.cargo/bin"
DEST_PATH="${DEST_DIR}/${BIN_NAME}"
TARGET_DIR="${ROOT_DIR}/target"

cd "${ROOT_DIR}"

echo "Building release binary..."
export CARGO_TARGET_DIR="${TARGET_DIR}"
cargo build --release -p tree-lang --bin "${BIN_NAME}"

echo "Installing to ${DEST_PATH} ..."
mkdir -p "${DEST_DIR}"
install -m 0755 "${TARGET_DIR}/release/${BIN_NAME}" "${DEST_PATH}"

echo "Done."
echo "Run: ${DEST_PATH} --help"
