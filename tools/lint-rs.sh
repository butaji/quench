#!/usr/bin/env bash
# Enforce rustfmt plus Clippy's function-size and complexity limits.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

tools/lint-size.sh

echo "=== rust source limits (40 lines/function, complexity 10) ==="
tools/lint-rust-limits.sh
echo "OK: Rust source limits"

echo "=== rustfmt check ==="
cargo fmt --check || { echo "FAIL: rustfmt — run 'cargo fmt'"; exit 1; }
echo "OK: rustfmt"

echo ""
echo "=== clippy (-D warnings) ==="
cargo clippy -p quench-node -- -D warnings -W clippy::too_many_lines -W clippy::cognitive_complexity || { echo "FAIL: clippy"; exit 1; }
echo "OK: clippy"

echo ""
echo "All Rust checks passed."
