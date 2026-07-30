#!/usr/bin/env bash
set -euo pipefail

USE_JSON=0
CURRENT_ONLY=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            USE_JSON=1
            shift
            ;;
        --current)
            CURRENT_ONLY=1
            shift
            ;;
        --help|-h)
            sed -n '1,160p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

python3 - "$USE_JSON" "$CURRENT_ONLY" <<'PY'
import json
import sys

try:
    use_json = int(sys.argv[1])
    current_only = int(sys.argv[2])
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

if use_json:
    if current_only:
        current_stage = next((s for s in stages if s.get('id') == current), None)
        print(json.dumps({
            'current_stage': current,
            'stage': current_stage,
        }, sort_keys=True))
        raise SystemExit(0)
    print(json.dumps({
        'current_stage': current,
        'stages': stages,
    }, sort_keys=True))
    raise SystemExit(0)

if current_only:
    stage = next((s for s in stages if s.get('id') == current), None)
    if stage is None:
        sys.stderr.write(f"error: current stage {current} not found in tasks/index.json\n")
        raise SystemExit(2)
    print(f"Current stage: {current}")
    print(f"Path:       {stage.get('path', '')}")
    print(f"Status:     {stage.get('status', 'unknown')}")
    print(f"Tests:      {stage.get('tests', 0)}")
    print(f"Failed:     {stage.get('failed', 0)}")
    raise SystemExit(0)

print(f"{'ID':>4} {'Status':>8}  {'Tests':>6}  Path")
print(f"{'----':>4} {'--------':>8}  {'-----':>6}  ------")

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
