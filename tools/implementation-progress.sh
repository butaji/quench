#!/usr/bin/env bash
set -euo pipefail

# Single command status snapshot for implementation progress.
# Usage:
#   bash tools/implementation-progress.sh
#   bash tools/implementation-progress.sh --next
#   bash tools/implementation-progress.sh --raw
#   bash tools/implementation-progress.sh --ci
#   bash tools/implementation-progress.sh --summary
RAW=0
INCLUDE_NEXT=0
CI=0
INCLUDE_SUMMARY=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --raw)
            RAW=1
            shift
            ;;
        --next)
            INCLUDE_NEXT=1
            shift
            ;;
        --ci)
            CI=1
            shift
            ;;
        --summary)
            INCLUDE_SUMMARY=1
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

if [[ "$RAW" -eq 1 ]]; then
    if [[ "$CI" -eq 1 ]]; then
        echo "error: --ci requires machine-readable output" >&2
        exit 1
    fi
    if [[ "$INCLUDE_SUMMARY" -eq 1 ]]; then
        echo "error: --summary requires machine-readable output" >&2
        exit 1
    fi
    bash tools/stage-status.sh --current
    if [[ "$INCLUDE_NEXT" -eq 1 ]]; then
        echo
        bash tools/stage-status.sh --next
    fi
    exit 0
fi

if ! CURRENT_JSON=$(bash tools/stage-status.sh --json --current); then
    echo "error: failed to read current stage status" >&2
    exit 1
fi
NEXT_JSON=''
if [[ "$INCLUDE_NEXT" -eq 1 ]]; then
    if ! NEXT_JSON=$(bash tools/stage-status.sh --json --next); then
        echo "error: failed to read next stage status" >&2
        exit 1
    fi
fi

STATS_JSON=''
if [[ "$INCLUDE_SUMMARY" -eq 1 ]]; then
    if ! STATS_JSON=$(bash tools/stage-status.sh --json); then
        echo "error: failed to read overall stage status" >&2
        exit 1
    fi
fi

python3 - "$CURRENT_JSON" "$NEXT_JSON" "$STATS_JSON" "$INCLUDE_NEXT" "$INCLUDE_SUMMARY" "$CI" <<'PY'
import json
import sys

current_json = sys.argv[1]
next_json = sys.argv[2]
stats_json = sys.argv[3]
include_next = sys.argv[4] == "1"
include_summary = sys.argv[5] == "1"
ci_mode = sys.argv[6] == "1"

current_payload = json.loads(current_json)
next_payload = json.loads(next_json) if next_json else None
stats_payload = json.loads(stats_json) if stats_json else None

current_stage = current_payload.get("stage") if isinstance(current_payload, dict) else None
current_status = current_stage.get("status") if isinstance(current_stage, dict) else None
has_pending = False
if include_next and isinstance(next_payload, dict):
    has_pending = bool(next_payload.get("next_stage") not in (None, 0, "0"))

out = {
    "implementation": {
        "current": current_payload,
        "has_pending": has_pending,
        "ci_ready": bool(current_status == "done" and not has_pending),
    }
}

if include_next:
    out["implementation"]["next"] = next_payload

if include_summary:
    stages = stats_payload.get('stages', []) if isinstance(stats_payload, dict) else []
    total_stages = len(stages)
    done_stages = sum(1 for s in stages if s.get('status') == 'done')
    total_tests = sum(s.get('tests', 0) for s in stages)
    done_tests = sum(s.get('tests', 0) for s in stages if s.get('status') == 'done')
    pending_tests = max(0, total_tests - done_tests)
    out["implementation"]["summary"] = {
        "stages_done": done_stages,
        "stages_total": total_stages,
        "tests_done": done_tests,
        "tests_total": total_tests,
        "tests_pending": pending_tests,
        "progress_percent": 0.0 if total_tests == 0 else round((done_tests * 100.0) / total_tests, 4),
    }

print(json.dumps(out, sort_keys=True))

if ci_mode:
    if current_status != "done" or has_pending:
        raise SystemExit(1)
    raise SystemExit(0)
PY
