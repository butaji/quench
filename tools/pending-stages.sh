#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

JSON=0
COUNT_ONLY=0
TOP=0
VERBOSE=0

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

python3 - "$JSON" "$COUNT_ONLY" "$TOP" "$VERBOSE" <<'PY'
import json
import sys

use_json = int(sys.argv[1])
count_only = int(sys.argv[2])
top = int(sys.argv[3])
verbose = int(sys.argv[4])

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
