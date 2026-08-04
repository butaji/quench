#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

exceptions="tasks/builtin-direct-bindings.txt"
failures=0

while IFS= read -r method; do
    [[ -z "$method" || "$method" == \#* ]] && continue
    if grep -R -n --include='*.rs' -E "[\"']__${method}[\"']" crates/quench-runtime/src/builtins >/tmp/quench-builtin-ownership.$$ 2>/dev/null; then
        if ! grep -Fxq "$method" "$exceptions"; then
            cat /tmp/quench-builtin-ownership.$$
            echo "duplicate builtin ownership: JS prototype method '$method' has a Rust hidden implementation" >&2
            failures=1
        fi
    fi
done < <(sed -nE 's/.*prototype\.([A-Za-z_$][A-Za-z0-9_$]*)[[:space:]]*=.*/\1/p' builtins/*.js | sort -u)

rm -f /tmp/quench-builtin-ownership.$$
exit "$failures"
