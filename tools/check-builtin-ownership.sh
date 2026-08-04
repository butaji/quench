#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

exceptions="tasks/builtin-direct-bindings.txt"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

failures=0

while IFS='|' read -r method reason extra; do
    [[ -z "$method" || "$method" == \#* ]] && continue
    if [[ -z "$reason" || -n "$extra" ]]; then
        echo "invalid direct binding record: $method|$reason|$extra" >&2
        failures=1
    fi
done < "$exceptions"

js_methods="$tmp_dir/js-methods"
rust_bindings="$tmp_dir/rust-bindings"
sed -nE 's/.*prototype\.([A-Za-z_$][A-Za-z0-9_$]*)[[:space:]]*=.*/\1/p' builtins/*.js | sort -u > "$js_methods"
grep -RhoE --include='*.rs' '__[A-Za-z_$][A-Za-z0-9_$]*' crates/quench-runtime/src/builtins \
    | sed -E 's/^__//' | sort -u > "$rust_bindings"

while IFS= read -r method; do
    [[ -z "$method" ]] && continue
    if grep -Fxq "$method" "$rust_bindings" && ! grep -Eq "^${method}[|]" "$exceptions"; then
        echo "duplicate builtin ownership: JS prototype method '$method' has a Rust hidden implementation" >&2
        failures=1
    fi
done < "$js_methods"

while IFS='|' read -r method reason extra; do
    [[ -z "$method" || "$method" == \#* ]] && continue
    if ! grep -Fxq "$method" "$js_methods"; then
        echo "stale direct binding record: $method|$reason" >&2
        failures=1
    fi
done < "$exceptions"

exit "$failures"
