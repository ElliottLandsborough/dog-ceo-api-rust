#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

echo "Building release binary..."
cargo build --release

echo "Running release binary..."
exec "${SCRIPT_DIR}/target/release/dog-ceo-rust" "$@"
