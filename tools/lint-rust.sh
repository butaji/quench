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

fail=0
while IFS= read -r file; do
    lines=$(wc -l < "$file")
    if (( lines > 500 )); then
        printf '%s: %d lines (maximum 500)\n' "$file" "$lines" >&2
        fail=1
    fi
done < <("$rg_bin" --files -g '*.rs' -g '!target/**' | sort)

while IFS= read -r file; do
    awk '
        function braces(text, clean) {
            clean = text
            gsub(/\/\/.*$/, "", clean)
            return gsub(/{/, "{", clean) - gsub(/}/, "}", clean)
        }
        /(^|[^[:alnum:]_])fn[[:space:]]+[[:alnum:]_]+[[:space:]]*\(/ {
            start = NR
            depth = 0
            active = 1
        }
        active {
            depth += braces($0)
            if (depth <= 0 && NR > start && $0 ~ /}/) {
                if (NR - start + 1 > 40) {
                    printf "%s:%d: function exceeds 40 lines (%d)\n", FILENAME, start, NR - start + 1 > "/dev/stderr"
                    fail = 1
                }
                active = 0
            }
        }
        END { exit fail }
    ' "$file" || fail=1
done < <("$rg_bin" --files -g '*.rs' -g '!target/**' | sort)

if (( fail != 0 )); then
    exit 1
fi

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -D clippy::cognitive_complexity
