#!/usr/bin/env bash
# Quick local pre-commit hook / check: formatting, clippy, and unit tests.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> [1/3] Checking formatting..."
cargo fmt --all --check || {
    echo "Formatting errors found. Run: cargo fmt --all"
    exit 1
}

echo "==> [2/3] Running Clippy on all targets..."
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [3/3] Running workspace unit & contract tests..."
cargo test --workspace

echo "==> Local pre-commit checks passed! Ready to push."
