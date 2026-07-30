#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json

with open('tasks/index.json') as f:
    data = json.load(f)

for item in data['stages']:
    print(item['id'])
PY

