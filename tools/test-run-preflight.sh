#!/usr/bin/env bash
set -euo pipefail

# Fast guard for stage test-run automation.
# Usage:
#   bash tools/test-run-preflight.sh
#   bash tools/test-run-preflight.sh --json
#   bash tools/test-run-preflight.sh --raw
#   bash tools/test-run-preflight.sh --stage 42

JSON=0
RAW=0
STAGE=""

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            JSON=1
            shift
            ;;
        --raw)
            RAW=1
            shift
            ;;
        --stage)
            if [[ "${2:-}" == "" ]]; then
                echo "error: --stage requires a stage id" >&2
                exit 1
            fi
            STAGE="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '1,120p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ "$JSON" -eq 1 && "$RAW" -eq 1 ]]; then
    echo "error: --json and --raw are mutually exclusive" >&2
    exit 1
fi

STAGE_DATA="$(bash tools/stage-status.sh --json --current)" || {
    echo "error: failed to read current stage" >&2
    exit 2
}

if [[ "$STAGE" == "" ]]; then
    STAGE="$(python3 - "$STAGE_DATA" <<'PY'
import json
import sys
payload = json.loads(sys.argv[1])
print(payload.get('current_stage', ''))
PY
)"
fi

python3 - "$STAGE_DATA" "$STAGE" "$JSON" "$RAW" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
stage = sys.argv[2]
json_mode = sys.argv[3] == "1"
raw_mode = sys.argv[4] == "1"

if not isinstance(payload, dict):
    print('error: invalid stage payload', file=sys.stderr)
    raise SystemExit(2)

info = payload.get('stage') if isinstance(payload, dict) else None
current = str(payload.get('current_stage', ''))
if not info:
    print('error: stage payload missing data', file=sys.stderr)
    raise SystemExit(2)

if stage and stage != current:
    print(f'error: requested stage {stage} does not match current stage {current}', file=sys.stderr)
    raise SystemExit(2)

status = info.get('status', '')
failed = int(info.get('failed', -1) or 0)
tests = int(info.get('tests', 0) or 0)
path = info.get('path', '')

if status not in {'pending', 'done', 'retry', 'running'}:
    print(f'error: unexpected status {status} for stage {current}', file=sys.stderr)
    raise SystemExit(2)

if tests <= 0:
    print(f'error: stage {current} has no test count', file=sys.stderr)
    raise SystemExit(2)

out = {
    'test_run_preflight': {
        'current_stage': current,
        'status': status,
        'tests': tests,
        'failed': failed,
        'path': path,
        'ready_for_test_run': True,
    }
}

if json_mode:
    print(json.dumps(out, sort_keys=True))
elif raw_mode:
    print(f"Current stage: {current}")
    print(f"Path:       {path}")
    print(f"Status:     {status}")
    print(f"Tests:      {tests}")
    print(f"Failed:     {failed}")
    print('Ready:      true')
else:
    print(f"{current}:{status}:{tests}:{failed}:{path}")
PY
