#!/usr/bin/env bash
set -euo pipefail

# Run a test plan and append execution metadata to a log.
# Usage:
#   bash tools/run-test-plan-log.sh [run-test-plan args...]
#   bash tools/run-test-plan-log.sh --log-file /tmp/plan.log --status --json
#   bash tools/run-test-plan-log.sh --log-file /tmp/plan.log --status-json
#   bash tools/run-test-plan-log.sh --status --json --raw
#   bash tools/run-test-plan-log.sh --summary-only --status --json

RUN_PLAN_LOG="${RUN_TEST_PLAN_LOG:-./.test262_plan_runs.log}"
SUMMARY_ONLY=0
RAW_OUTPUT=0
ARGS=("$@")

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --log-file)
            RUN_PLAN_LOG="$2"
            shift 2
            ;;
        --summary-only)
            SUMMARY_ONLY=1
            shift
            ;;
        --raw)
            RAW_OUTPUT=1
            shift
            ;;
        -h|--help)
            sed -n '1,200p' "$0"
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
set +e
PLAN_OUTPUT="$(bash tools/run-test-plan.sh "${ARGS[@]}" 2>&1)"
EXIT_CODE=$?
set -e

if [[ "$SUMMARY_ONLY" -eq 0 ]]; then
    printf '%s' "$PLAN_OUTPUT"
fi

PLAN_OUTPUT_B64="$(printf '%s' "$PLAN_OUTPUT" | base64 -w0)"

python3 - "$RUN_PLAN_LOG" "$EXIT_CODE" "$SUMMARY_ONLY" "$RAW_OUTPUT" "$PLAN_OUTPUT_B64" <<'PY'
import json
import sys
import base64
from datetime import datetime
from pathlib import Path

log_path = Path(sys.argv[1])
exit_code = int(sys.argv[3])
summary_only = sys.argv[4] == "1"
raw_output = sys.argv[5] == "1"
out_text = base64.b64decode(sys.argv[6]).decode("utf-8", errors="replace")


def extract_last_json_object(text: str):
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    for line in reversed(lines):
        candidate = line
        if not candidate.startswith("{") or not candidate.endswith("}"):
            continue
        try:
            return json.loads(candidate)
        except json.JSONDecodeError:
            pass
    return None

summary = {
    "raw_output": raw_output,
}

payload = None
try:
    payload = json.loads(out_text)
except json.JSONDecodeError:
    payload = extract_last_json_object(out_text)
    if payload is None:
        if raw_output and not summary_only:
            if len(out_text) > 4096:
                summary["raw"] = out_text[-4096:]
                summary["raw_truncated"] = True
            else:
                summary["raw"] = out_text.strip()
                summary["raw_truncated"] = False
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

if [[ "$EXIT_CODE" -ne 0 ]]; then
    exit "$EXIT_CODE"
fi
