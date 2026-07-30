#!/usr/bin/env bash
set -euo pipefail

# Compact status summary for the current stage test-run.
# Usage:
#   bash tools/test-run-status-summary.sh
#   bash tools/test-run-status-summary.sh --json
#   bash tools/test-run-status-summary.sh --blocker

JSON=0
BLOCKER_ONLY=0
while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            JSON=1
            shift
            ;;
        --blocker)
            BLOCKER_ONLY=1
            shift
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

CURRENT_JSON="$(bash tools/stage-status.sh --json --current)"
NEXT_JSON="$(bash tools/stage-status.sh --json --next)"

python3 - "$CURRENT_JSON" "$NEXT_JSON" "$JSON" "$BLOCKER_ONLY" <<'PY'
import json
import sys
current_payload = json.loads(sys.argv[1]) if sys.argv[1] else {}
next_payload = json.loads(sys.argv[2]) if sys.argv[2] else {}
json_mode = sys.argv[3] == "1"
blocker_only = sys.argv[4] == "1"

current_stage = current_payload.get('stage', {}) if isinstance(current_payload, dict) else {}
next_stage = next_payload.get('next_stage') if isinstance(next_payload, dict) else None

failed = int(current_stage.get('failed', 0) or 0)
blocked = failed > 0

if json_mode:
    status = {
        'current_stage': current_payload.get('current_stage'),
        'status': current_stage.get('status'),
        'path': current_stage.get('path'),
        'failed': failed,
        'tests': current_stage.get('tests'),
        'is_blocked': blocked,
        'has_next': bool(next_stage not in (None, 0, '0')),
    }
    if not blocker_only or blocked:
        print(json.dumps({'test_run_status_summary': status}, sort_keys=True))
    raise SystemExit(1 if blocker_only and blocked else 0)

if blocker_only:
    if blocked:
        print(f"blocker: current stage has {failed} failures")
        raise SystemExit(1)
    print("no blocker: failed tests = 0")
    raise SystemExit(0)

current_stage_id = current_payload.get('current_stage', '')
current_status = current_stage.get('status', '')
current_path = current_stage.get('path', '')
current_failed = int(current_stage.get('failed', 0) or 0)
current_tests = int(current_stage.get('tests', 0) or 0)
has_next = 1 if next_stage not in (None, 0, '0') else 0

print(f"{current_stage_id}\t{current_status}\t{current_failed}\t{current_tests:>6}\t{has_next:>6}\t{current_path}")
PY

if [[ "$JSON" -eq 1 ]]; then
    exit 0
fi

if [[ "$BLOCKER_ONLY" -eq 1 ]]; then
    exit 0
fi

# Should not reach here because python script sets the required exit code.
exit 0
