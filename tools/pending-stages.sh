#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

JSON=0
COUNT_ONLY=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            JSON=1
            shift
            ;;
        --count)
            COUNT_ONLY=1
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

python3 - "$JSON" "$COUNT_ONLY" <<'PY'
import json
import sys

use_json = int(sys.argv[1])
count_only = int(sys.argv[2])

try:
    with open('tasks/index.json') as f:
        data = json.load(f)
except (OSError, json.JSONDecodeError) as exc:
    sys.stderr.write(f'error: failed to read/parse tasks/index.json: {exc}\n')
    raise SystemExit(1)

stages = data.get('stages', [])
pending = [s for s in stages if s.get('status') != 'done']
if count_only:
    print(len(pending))
    raise SystemExit(0)

if use_json:
    print(json.dumps({'count': len(pending), 'stages': pending}, sort_keys=True))
    raise SystemExit(0)

for stage in pending:
    print(stage.get('id', ''))
PY
