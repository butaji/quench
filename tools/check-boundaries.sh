#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime_dir="$root_dir/crates/quench-runtime"

if rg -n -i 'test262|harness|conformance|stage[[:space:]_-]*[0-9]|\$262' \
    "$runtime_dir/src" "$runtime_dir/Cargo.toml"; then
    printf 'quench-runtime must remain independent of test262 and its harness\n' >&2
    exit 1
fi

if rg -n 'quench-test262|tests/test262' "$runtime_dir"; then
    printf 'quench-runtime must not depend on quench-test262 or tests/test262\n' >&2
    exit 1
fi
