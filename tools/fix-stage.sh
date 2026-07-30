#!/usr/bin/env bash
# Run a stage, print the first failure, and open it with run-test for fast triage.
# Usage:
#   TEST262_STAGE=27 bash tools/fix-stage.sh
#   bash tools/fix-stage.sh          # uses TEST262_STAGE if set, else current_stage
#
# Optional:
#   TEST262_QUICK=1 ... keeps first-failure feedback focused.

set -euo pipefail

cd "$(dirname "$0")/.."

STAGE=${TEST262_STAGE:-$(python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])")}
MODE=${1:-}

if [ "$MODE" = "--help" ] || [ "$MODE" = "-h" ]; then
    sed -n '1,120p' "$0"
    exit 0
fi

echo "[fix-stage] Running stage $STAGE..."

OUTFILE=$(mktemp)
trap 'rm -f "$OUTFILE"' EXIT

if ! TEST262_STAGE=$STAGE TEST262_DIGEST=1 TEST262_QUICK=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture > "$OUTFILE" 2>&1; then
    echo "[fix-stage] Stage $STAGE failed. Extracting first failing test..."
    FIRST_FAIL=$(python3 - "$OUTFILE" <<'PY'
import json
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
text = path.read_text(errors='ignore')

def first_path_from_json(payload: str) -> str:
    try:
        data = json.loads(payload)
    except json.JSONDecodeError:
        return ""

    for group in data.get("groups", []):
        for key in ("sample_paths", "samples"):
            entries = group.get(key) or []
            if not isinstance(entries, list) or not entries:
                continue
            first = entries[0]
            if isinstance(first, str):
                return first
            if isinstance(first, dict):
                maybe = first.get("path") or first.get("test") or first.get("file")
                if maybe:
                    return str(maybe)
    return ""

def first_path_from_lines(payload: str) -> str:
    for line in payload.splitlines():
        m = re.search(r'"sample_paths"\\s*:\\s*\\[\\s*"([^"]+)"', line)
        if m:
            return m.group(1)
        m = re.search(r'"path"\\s*:\\s*"([^"]+\\.js)"', line)
        if m:
            return m.group(1)
    return ""

path_text = first_path_from_json(text)
if not path_text:
    path_text = first_path_from_lines(text)
print(path_text)
PY
    )
    if [ -n "$FIRST_FAIL" ]; then
        echo "[fix-stage] Re-running first failure with full harness diagnostics:"
        echo "  $FIRST_FAIL"
        cargo run --bin run-test -- --show-script "$FIRST_FAIL"
    else
        echo "[fix-stage] Could not parse first failing test path; showing tail:"
        tail -80 "$OUTFILE"
    fi
    exit 1
fi

echo "[fix-stage] Stage $STAGE passed."
