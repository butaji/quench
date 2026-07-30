#!/usr/bin/env bash
set -euo pipefail

STAGE="${1:-$(bash tools/current-stage.sh)}"

python3 - "$STAGE" <<'PY'
import json
import sys

stage = int(sys.argv[1])
with open('tasks/index.json') as f:
    data = json.load(f)
for item in data['stages']:
    if item.get('id') == stage:
        print(item.get('path', ''))
        break
PY

