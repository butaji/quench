#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${TEST262_STAGE:-}" ]]; then
    echo "$TEST262_STAGE"
else
    python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])"
fi

