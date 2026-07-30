#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${TEST262_STAGE:-}" ]]; then
    if [[ ! "$TEST262_STAGE" =~ ^[0-9]+$ ]]; then
        echo "error: TEST262_STAGE must be numeric" >&2
        exit 2
    fi
    echo "$TEST262_STAGE"
    exit 0
fi

CURRENT_STAGE=$(python3 - <<'PY'
import json
import sys

try:
    with open('tasks/index.json') as f:
        data = json.load(f)
    print(data.get('current_stage', ''))
except (OSError, json.JSONDecodeError) as exc:
    print(f'__ERROR__:{exc}')
    raise SystemExit(2)
PY
)

if [[ "$CURRENT_STAGE" == __ERROR__* ]]; then
    echo "error: failed to read current_stage from tasks/index.json" >&2
    exit 2
fi

if [[ -z "$CURRENT_STAGE" || ! "$CURRENT_STAGE" =~ ^[0-9]+$ ]]; then
    echo "error: invalid current_stage in tasks/index.json" >&2
    exit 2
fi

echo "$CURRENT_STAGE"
