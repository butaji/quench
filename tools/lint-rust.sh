#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

rg_bin="$(command -v rg || true)"
for candidate in /opt/homebrew/bin/rg /usr/local/bin/rg /usr/bin/rg; do
    if [[ -x "$candidate" ]] && "$candidate" --version >/dev/null 2>&1; then
        rg_bin="$candidate"
        break
    fi
done
if [[ -z "$rg_bin" ]]; then
    printf 'ripgrep is required for Rust linting\n' >&2
    exit 1
fi

tools/check-boundaries.sh

cargo fmt --all -- --check
# The host implementation is covered by its dedicated Node oracle; the
# node-test fixture crate pulls it into workspace resolution, so exclude both
# host packages from this interpreter-focused style gate.
cargo clippy --workspace --exclude quench-node --exclude quench-node-test --all-targets -- -D warnings
