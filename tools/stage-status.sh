#!/bin/bash
# Show the stage workflow status recorded in index.json.
# Usage: bash tools/stage-status.sh

set -e
cd "$(dirname "$0")/.."

python3 -c "
import json

with open('tasks/index.json') as file:
    data = json.load(file)

current = data['current_stage']
print(f\"{'ID':>4} {'Status':>12}  Path\")
print(f\"{'──':>4} {'──────':>12}  ────\")
for stage in data['stages']:
    marker = '>>>' if stage['id'] == current else '   '
    print(f\"{marker} {stage['id']:3d} {stage['status']:>12}  {stage['path']}\")
print()
print(f\"Current: Stage {current}\")
"
