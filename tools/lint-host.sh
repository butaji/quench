#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "=== rustfmt (host) ==="
cargo fmt --manifest-path "$root/Cargo.toml" --check

echo "=== clippy (quench-node host) ==="
cargo clippy --manifest-path "$root/Cargo.toml" -p quench-node -- \
  -D warnings -W clippy::too_many_lines -W clippy::cognitive_complexity

echo "Host Rust checks passed."
