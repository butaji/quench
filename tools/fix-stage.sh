#!/usr/bin/env bash
# Run a stage, print failing test(s), and open the first failure with run-test for triage.
# Usage:
#   TEST262_STAGE=27 bash tools/fix-stage.sh
#   bash tools/fix-stage.sh          # uses TEST262_STAGE if set, else current_stage
#
# Optional:
#   TEST262_QUICK=1 ... keeps first-failure feedback focused.

set -euo pipefail

cd "$(dirname "$0")/.."

STAGE="$(bash tools/current-stage.sh)"
MODE=${1:-}

if [ "$MODE" = "--help" ] || [ "$MODE" = "-h" ]; then
    sed -n '1,140p' "$0"
    exit 0
fi

echo "[fix-stage] Running stage $STAGE..."

OUTFILE=$(mktemp)
trap 'rm -f "$OUTFILE"' EXIT

if ! TEST262_STAGE=$STAGE TEST262_DIGEST=1 TEST262_QUICK=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture > "$OUTFILE" 2>&1; then
    echo "[fix-stage] Stage $STAGE failed. Extracting first failing test..."

    FAILS=()
    while IFS= read -r path; do
        [ -n "$path" ] && FAILS+=("$path")
    done < <(python3 - "$OUTFILE" <<'PY'
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text(errors='ignore')

patterns = [
    re.compile(r'"sample_paths"\s*:\s*\[\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r'"path"\s*:\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r'"test"\s*:\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r'"file"\s*:\s*"([^"]+\.js)"', re.DOTALL),
]

results = []
for pat in patterns:
    for match in pat.finditer(text):
        candidate = match.group(1)
        if candidate not in results:
            results.append(candidate)

if not results:
    for match in re.finditer(r"FAILED [^\n]*tests/.+?\.js", text):
        file_match = re.search(r"(tests/.+\.js)", match.group(0))
        if file_match:
            candidate = file_match.group(1)
            if candidate not in results:
                results.append(candidate)

for path in results[:12]:
    print(path)
PY
    )

    if [ ${#FAILS[@]} -eq 0 ]; then
        echo "[fix-stage] Could not parse first failing test path; showing tail:"
        tail -80 "$OUTFILE"
        exit 1
    fi

    FIRST_FAIL=${FAILS[0]}
    echo "[fix-stage] Found ${#FAILS[@]} candidate failing path(s)."
    echo "[fix-stage] Top failures:"
    for path in "${FAILS[@]:0:5}"; do
        echo "  - $path"
    done

    echo "[fix-stage] Re-running first failure with full harness diagnostics:"
    echo "  $FIRST_FAIL"
    cargo run --bin run-test -- --show-script "$FIRST_FAIL"
    exit 1
fi

echo "[fix-stage] Stage $STAGE passed."
