#!/usr/bin/env bash
set -euo pipefail

STAGE="${1:-$(bash tools/current-stage.sh)}"

if [[ ! "$STAGE" =~ ^[0-9]+$ ]]; then
    echo "error: stage must be numeric: $STAGE" >&2
    exit 2
fi

PY_OUTPUT=$(python3 - "$STAGE" <<'PY'
import json
import sys

stage = int(sys.argv[1])
with open('tasks/index.json') as f:
    data = json.load(f)
for item in data['stages']:
    if item.get('id') == stage:
        print(item.get('tests', 0))
        break
else:
    sys.exit(1)
PY
)

if [[ -z "$PY_OUTPUT" ]]; then
    echo "error: stage $STAGE not found in tasks/index.json" >&2
    exit 2
fi
printf '%s\n' "$PY_OUTPUT"
