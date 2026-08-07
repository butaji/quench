#!/bin/bash
# Quick overview of all stages status.
# Usage: bash tools/stage-status.sh
#
# Shows pass rate for each stage based on index.json status,
# plus quick-test for the current stage.

cd "$(dirname "$0")/.."

python3 -c "
import json
with open('tasks/index.json') as f:
    d = json.load(f)

current = d['current_stage']
done = 0
pending = 0
total_tests = 0
done_tests = 0

print(f\"{'ID':>4} {'Status':>8}  {'Tests':>6}  Path\")
print(f\"{'──':>4} {'──────':>8}  {'─────':>6}  ────\")

for s in d['stages']:
    marker = '>>>' if s['id'] == current else '   '
    total_tests += s['tests']
    if s['status'] == 'done':
        done += 1
        done_tests += s['tests']
    else:
        pending += 1
    print(f'{marker} {s[\"id\"]:3d} {s[\"status\"]:>8}  {s[\"tests\"]:>6}  {s[\"path\"]}')

print()
print(f'Done: {done}/{len(d[\"stages\"])} stages ({done_tests}/{total_tests} tests)')
print(f'Pending: {pending} stages ({total_tests - done_tests} tests)')
print(f'Current: Stage {current}')
print(f'Progress: {done_tests * 100 / total_tests:.1f}%')
" 2>/dev/null
