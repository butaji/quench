#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

JSON=0
COUNT_ONLY=0
TOP=0
VERBOSE=0
SUMMARY=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            JSON=1
            shift
            ;;
        --top)
            if [[ "${2:-}" == "" || ! "${2:-}" =~ ^[0-9]+$ ]]; then
                echo "error: --top requires a numeric argument" >&2
                exit 1
            fi
            TOP="$2"
            shift 2
            ;;
        --count)
            COUNT_ONLY=1
            shift
            ;;
        --verbose|-v)
            VERBOSE=1
            shift
            ;;
        --summary)
            SUMMARY=1
            shift
            ;;
        -h|--help)
            sed -n '1,160p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

python3 - "$JSON" "$COUNT_ONLY" "$TOP" "$VERBOSE" "$SUMMARY" <<'PY'
import json
import sys

use_json = int(sys.argv[1])
count_only = int(sys.argv[2])
top = int(sys.argv[3])
verbose = int(sys.argv[4])
summary = int(sys.argv[5])

try:
    with open('tasks/index.json') as f:
        data = json.load(f)
except (OSError, json.JSONDecodeError) as exc:
    sys.stderr.write(f'error: failed to read/parse tasks/index.json: {exc}\n')
    raise SystemExit(1)

stages = data.get('stages', [])
pending = [s for s in stages if s.get('status') != 'done']
pending.sort(key=lambda s: (-int(s.get('failed', 0)), s.get('id', 0)))
if top > 0:
    pending = pending[:top]
done_stages = [s for s in stages if s.get('status') == 'done']

if summary:
    total_pending_tests = sum(int(s.get('tests', 0)) for s in stages if s.get('status') != 'done')
    total_done_tests = sum(int(s.get('tests', 0)) for s in stages if s.get('status') == 'done')
    total_pending_failed = sum(int(s.get('failed', 0)) for s in stages if s.get('status') != 'done')
    total_done = len(done_stages)
    total_stages = len(stages)
    total_tests = total_pending_tests + total_done_tests
    payload = {
        'pending_stages': len([s for s in stages if s.get('status') != 'done']),
        'done_stages': total_done,
        'total_stages': total_stages,
        'pending_tests': total_pending_tests,
        'done_tests': total_done_tests,
        'total_tests': total_tests,
        'pending_failed': total_pending_failed,
    }
    if use_json:
        print(json.dumps({'summary': payload}, sort_keys=True))
        raise SystemExit(0)
    print(f"Pending stages: {payload['pending_stages']}")
    print(f"Done stages:    {payload['done_stages']}")
    print(f"Total stages:   {payload['total_stages']}")
    print(f"Pending tests:  {payload['pending_tests']}")
    print(f"Done tests:     {payload['done_tests']}")
    print(f"Total tests:    {payload['total_tests']}")
    print(f"Pending failed: {payload['pending_failed']}")
    raise SystemExit(0)

if count_only:
    print(len(pending) if top == 0 else min(top, len(pending)))
    raise SystemExit(0)

if use_json:
    print(json.dumps({'count': len(pending), 'stages': pending}, sort_keys=True))
    raise SystemExit(0)

if verbose:
    print(f"{'ID':>4} {'Failed':>6} {'Tests':>5} Path")
    print(f"{'----':>4} {'------':>6} {'-----':>5} ----")
    for stage in pending:
        sid = str(stage.get('id', ''))
        print(f"{sid:>4} {stage.get('failed', 0):>6} {stage.get('tests', 0):>5} {stage.get('path', '')}")
    raise SystemExit(0)

for stage in pending:
    print(stage.get('id', ''))
PY
