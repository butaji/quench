#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime_dir="$root_dir/crates/quench-runtime"
rg_bin="$(command -v rg || true)"
for candidate in /opt/homebrew/bin/rg /usr/local/bin/rg /usr/bin/rg; do
    if [[ -x "$candidate" ]] && "$candidate" --version >/dev/null 2>&1; then
        rg_bin="$candidate"
        break
    fi
done
if [[ -z "$rg_bin" ]]; then
    printf 'ripgrep is required for boundary checks\n' >&2
    exit 1
fi

if "$rg_bin" -n -i 'test262|harness|conformance|stage[[:space:]_-]*[0-9]|\$262' \
    "$runtime_dir/src" "$runtime_dir/Cargo.toml"; then
    printf 'quench-runtime must remain independent of test262 and its harness\n' >&2
    exit 1
fi

if "$rg_bin" -n 'quench-test262|tests/test262' "$runtime_dir"; then
    printf 'quench-runtime must not depend on quench-test262 or tests/test262\n' >&2
    exit 1
fi
