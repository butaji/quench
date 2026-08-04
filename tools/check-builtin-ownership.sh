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

# A Rust implementation must declare its public builtin name with the
# `@builtin-rust` marker. This avoids guessing from arbitrary identifiers and
# makes an untracked Rust implementation fail closed.
grep -RhoE --include='*.rs' '@builtin-rust[[:space:]]+[A-Za-z_$][A-Za-z0-9_$]*' \
    crates/quench-runtime/src/builtins \
    | sed -E 's/.*[[:space:]]//' | sort -u > "$rust_bindings"

# Every JS prototype assignment is an ownership declaration. Keep the
# receiver in the key so Array.prototype.map and Map.prototype.get do not
# collide merely because their method names match.
sed -nE 's/^[[:space:]]*([A-Za-z_$][A-Za-z0-9_$]*\.prototype\.[A-Za-z_$][A-Za-z0-9_$]*)[[:space:]]*=.*/\1/p' \
    builtins/*.js | sort > "$js_methods"

while IFS= read -r owner; do
    [[ -z "$owner" ]] && continue
    method="${owner##*.}"
    if grep -Fxq "$method" "$rust_bindings" && ! grep -Eq "^${method}[|]" "$exceptions"; then
        echo "duplicate builtin ownership: '$owner' has a Rust implementation" >&2
        failures=1
    fi
done < <(cut -d. -f1-3 "$js_methods" | sort -u)

while IFS='|' read -r method reason extra; do
    [[ -z "$method" || "$method" == \#* ]] && continue
    if ! grep -Fxq "$method" "$js_methods"; then
        echo "stale direct binding record: $method|$reason" >&2
        failures=1
    fi
done < "$exceptions"

exit "$failures"
