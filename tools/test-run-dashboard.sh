#!/usr/bin/env bash
set -euo pipefail

# End-to-end stage test-run dashboard.
# Usage:
#   bash tools/test-run-dashboard.sh
#   bash tools/test-run-dashboard.sh --json
#   bash tools/test-run-dashboard.sh --assert-ready

JSON=0
ASSERT_READY=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            JSON=1
            shift
            ;;
        --assert-ready)
            ASSERT_READY=1
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

ALL_JSON=$(bash tools/stage-status.sh --json)

python3 - "$ALL_JSON" "$JSON" "$ASSERT_READY" <<'PY'
import json
import sys

all_payload = json.loads(sys.argv[1])
json_mode = sys.argv[2] == "1"
assert_ready = sys.argv[3] == "1"

if not isinstance(all_payload, dict):
    print("error: invalid stage payload", file=sys.stderr)
    raise SystemExit(2)

stages = all_payload.get("stages", []) if isinstance(all_payload, dict) else []
current_stage = all_payload.get("current_stage", None)
current = next((s for s in stages if s.get("id") == current_stage), {})
next_entry = next((s for s in stages if s.get("status") != "done" and s.get("id", 0) > (current_stage or 0)), None)
next_id = next_entry.get('id') if isinstance(next_entry, dict) else None

status = current.get("status", "unknown")
failed = int(current.get("failed", 0) or 0)
tests = int(current.get("tests", 0) or 0)
stage_id = current_stage

all_done = all(s.get("status") == "done" for s in stages) if stages else False
has_pending = any(s.get("status") != "done" for s in stages) if stages else True
has_next = bool(next_id not in (None, 0, "0", ""))

ready = status in {"pending", "retry", "running"} and tests > 0
blocked = failed > 0

dashboard = {
    "current": {
        "stage": stage_id,
        "status": status,
        "failed": failed,
        "tests": tests,
        "path": current.get("path"),
    },
    "next": {
        "stage": next_id,
        "path": next_entry.get("path") if isinstance(next_entry, dict) else None,
        "status": next_entry.get("status") if isinstance(next_entry, dict) else None,
    } if has_next else None,
    "metrics": {
        "total_stages": len(stages),
        "has_pending_stages": has_pending,
        "all_stages_done": all_done,
        "has_next": has_next,
    },
    "signals": {
        "ready_for_test_run": ready,
        "blocked": blocked,
        "can_run": ready and not blocked,
    },
}

if json_mode:
    print(json.dumps({"test_run_dashboard": dashboard}, sort_keys=True))
else:
    print(
        f"{stage_id}\t{status}\t{tests}\t{failed}\t" +
        f"{1 if has_next else 0}\t{1 if blocked else 0}\t{1 if ready else 0}"
    )

if assert_ready and (not ready or blocked):
    raise SystemExit(1)
PY
