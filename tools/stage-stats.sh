#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json
import sys

try:
    with open('tasks/index.json') as f:
        data = json.load(f)
except (OSError, json.JSONDecodeError) as exc:
    print(f'__ERROR__:{exc}')
    raise SystemExit(1)

stages = data.get('stages', [])
current = data.get('current_stage', '')
if not current or current is None:
    print('__ERROR__:current_stage missing')
    raise SystemExit(1)

print(f'{"ID":>4} {"Status":>8}  {"Tests":>6}  Path')
print(f'{"——":>4} {"——————":>8}  {"─────":>6}  ────')

for s in stages:
    marker = '>>> ' if s.get('id') == current else '    '
    print(f"{marker}{s['id']:3d} {str(s['status']):>8}  {s['tests']:>6}  {s['path']}")

total = sum(s.get('tests', 0) for s in stages)
done_tests = sum(s.get('tests', 0) for s in stages if s.get('status') == 'done')
done = sum(1 for s in stages if s.get('status') == 'done')
pending = len(stages) - done

print()
print(f'Done: {done}/{len(stages)} stages ({done_tests}/{total} tests)')
print(f'Pending: {pending} stages ({total - done_tests} tests)')
print(f'Current: Stage {current}')
print(f'Progress: {done_tests * 100 / total:.1f}%')
PY
