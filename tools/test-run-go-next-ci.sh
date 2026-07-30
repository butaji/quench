#!/usr/bin/env bash
set -euo pipefail

# CI-safe one-shot next-stage readiness gate.
# Usage:
#   bash tools/test-run-go-next-ci.sh [--json | --json-only] [--by-ratio --top 5]
#   bash tools/test-run-go-next-ci.sh --json-only --stage 42

JSON=0
ARGS=("--assert-ready")
DRYRUN_OUTPUT=""
DRYRUN_ERRFILE=""

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json|--json-only)
            JSON=1
            shift
            ;;
        -h|--help)
            sed -n '1,220p' "$0"
            exit 0
            ;;
        *)
            ARGS+=("$1")
            shift
            ;;
    esac
done

if [[ "$JSON" -eq 1 ]]; then
    DRYRUN_ERRFILE="$(mktemp)"
    set +e
    DRYRUN_OUTPUT="$(bash tools/test-run-go-next-dryrun.sh --print-json "${ARGS[@]}" 2>"$DRYRUN_ERRFILE")"
    RC=$?
    set -e
    DRYRUN_ERR="$(cat "$DRYRUN_ERRFILE")"
    rm -f "$DRYRUN_ERRFILE"

    if [[ "$RC" -eq 0 ]]; then
        python3 - "$DRYRUN_OUTPUT" "$DRYRUN_ERR" "$RC" <<'PY'
import json
import sys

payload = sys.argv[1]
error = sys.argv[2].strip()
rc = int(sys.argv[3])

try:
    payload_obj = json.loads(payload) if payload else {}
except Exception:
    payload_obj = {}

obj = {
    "ci": {"ready": True},
    "payload": payload_obj if isinstance(payload_obj, dict) else {},
    "error": None,
}
print(json.dumps(obj))
PY
    else
        python3 - "$DRYRUN_ERR" "$RC" <<'PY'
import json
import sys

error = (sys.argv[1] or "").strip()
rc = int(sys.argv[2])

obj = {
    "ci": {"ready": False},
    "payload": {},
    "error": error,
}
print(json.dumps(obj))
PY
    fi
    exit $RC
else
    bash tools/test-run-go-next-dryrun.sh "${ARGS[@]}"
fi
