#!/usr/bin/env bash
set -euo pipefail

# Run a test plan and append execution metadata to a log.
# Usage:
#   bash tools/run-test-plan-log.sh [run-test-plan args...]
#   bash tools/run-test-plan-log.sh --log-file /tmp/plan.log --status --json

RUN_PLAN_LOG="${RUN_TEST_PLAN_LOG:-./.test262_plan_runs.log}"
ARGS=("$@")

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --log-file)
            RUN_PLAN_LOG="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '1,160p' "$0"
            exit 0
            ;;
        *)
            break
            ;;
    esac

done

if [[ -z "${RUN_PLAN_LOG}" ]]; then
    echo "error: --log-file requires a path" >&2
    exit 1
fi

mkdir -p "$(dirname "${RUN_PLAN_LOG}")"
TMP_OUTPUT="$(mktemp)"
set +e
bash tools/run-test-plan.sh "${ARGS[@]}" > "$TMP_OUTPUT" 2>&1
EXIT_CODE=$?
set -e

cat "$TMP_OUTPUT"

python3 - "$RUN_PLAN_LOG" "$TMP_OUTPUT" "$EXIT_CODE" <<'PY'
import json
import sys
from datetime import datetime
from pathlib import Path

log_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
exit_code = int(sys.argv[3])
out_text = output_path.read_text()

summary = {}
try:
    payload = json.loads(out_text)
except json.JSONDecodeError:
    summary = {"raw_output": out_text.strip()}
else:
    run_test_plan = payload.get("run-test-plan") if isinstance(payload, dict) else None
    if isinstance(run_test_plan, dict):
        summary["mode"] = run_test_plan.get("mode")
        status_payload = run_test_plan.get("status_payload", {})
        if isinstance(status_payload, dict) and summary["mode"] == "single":
            summary["stage"] = status_payload.get("stage")
            summary["source"] = status_payload.get("source")
        elif isinstance(status_payload, dict) and summary["mode"] == "batch":
            batch = status_payload.get("run-pending-batch", {})
            if isinstance(batch, dict):
                summary["count"] = batch.get("count")
                summary["top"] = batch.get("top")
                summary["stages"] = len(batch.get("stages", []) or [])

entry = {
    "ts": datetime.utcnow().isoformat() + "Z",
    "exit_code": exit_code,
    "status": "success" if exit_code == 0 else "failure",
    "summary": summary,
}

with log_path.open("a") as f:
    f.write(json.dumps(entry, sort_keys=True) + "\n")
PY

rm -f "$TMP_OUTPUT"

if [[ "$EXIT_CODE" -ne 0 ]]; then
    exit "$EXIT_CODE"
fi
