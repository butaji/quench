#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
status=0

while IFS= read -r -d '' file; do
    lines="$(wc -l < "$file")"
    if (( lines > 500 )); then
        printf '%s: %s lines (limit 500)\n' "${file#"$root/"}" "$lines" >&2
        status=1
    fi
done < <(find "$root/crates" "$root/tools" "$root/builtins" -type f \( -name '*.rs' -o -name '*.js' \) -print0)

python3 - "$root" <<'PY'
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
status = 0
function_start = re.compile(r"(?:function(?:\s+\w+)?\s*\([^)]*\)|(?:get|set)\s+[#\w$]+\s*\([^)]*\)|(?:async\s+)?(?:\w+|[#\w$]+)\s*\([^)]*\)\s*\{)")
complexity = re.compile(r"\b(?:if|for|while|catch|case)\b|&&|\|\||\?")

for base in (root / "crates", root / "tools", root / "builtins"):
    for path in base.rglob("*.js"):
        lines = path.read_text(encoding="utf-8").splitlines()
        index = 0
        while index < len(lines):
            match = function_start.search(lines[index])
            if not match:
                index += 1
                continue
            depth = 0
            started = False
            end = index
            score = 0
            for cursor in range(index, len(lines)):
                line = lines[cursor]
                depth += line.count("{") - line.count("}")
                started |= "{" in line
                if cursor > index or started:
                    score += len(complexity.findall(line))
                if started and depth <= 0:
                    end = cursor
                    break
            length = end - index + 1
            if length > 40:
                print(f"{path.relative_to(root)}:{index + 1}: {length} lines (function limit 40)", file=sys.stderr)
                status = 1
            if score > 10:
                print(f"{path.relative_to(root)}:{index + 1}: complexity {score} (limit 10)", file=sys.stderr)
                status = 1
            index = max(index + 1, end + 1)

sys.exit(status)
PY

cargo clippy -p quench-runtime --all-targets
