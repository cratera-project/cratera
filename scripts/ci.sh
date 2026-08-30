#!/usr/bin/env bash
# CI verification script: formatting, clippy on all targets, workspace tests, and release builds.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> [1/6] Checking formatting..."
cargo fmt --all --check

echo "==> [2/6] Running Clippy on all targets..."
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [3/6] Validating languages.toml manifest..."
python3 -c 'import tomllib; data = tomllib.load(open("languages.toml", "rb")); langs = data.get("languages", data); print(f"Valid TOML: {len(langs)} languages configured")'

if command -v shellcheck >/dev/null 2>&1; then
  echo "==> [4/6] Linting shell scripts (shellcheck)..."
  shellcheck scripts/*.sh examples/*.sh
else
  echo "==> [4/6] Shellcheck skipped (not installed on host)"
fi

echo "==> [5/6] Running workspace unit & contract tests..."
cargo test --workspace

echo "==> [6/6] Building release host binary & guest agent (musl static)..."
cargo build --release -p cratera-api
rustup target add x86_64-unknown-linux-musl 2>/dev/null || true
cargo build --release -p cratera-guest-agent --target x86_64-unknown-linux-musl

echo "==> All local CI checks passed successfully!"
