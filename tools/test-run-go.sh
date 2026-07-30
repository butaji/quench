#!/usr/bin/env bash
set -euo pipefail

# Single command to validate readiness and run a stage test-run.
# Usage:
#   bash tools/test-run-go.sh
#   bash tools/test-run-go.sh --ready
#   bash tools/test-run-go.sh --run
#   bash tools/test-run-go.sh --run --json
#   bash tools/test-run-go.sh --run --no-preflight

JSON=0
RUN=0
READY=0
NOPREFLIGHT=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            JSON=1
            shift
            ;;
        --run)
            RUN=1
            shift
            ;;
        --ready)
            READY=1
            shift
            ;;
        --no-preflight)
            NOPREFLIGHT=1
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

if [[ "$JSON" -eq 1 ]]; then
    DASH_ARGS=(--json)
    if [[ "$READY" -eq 1 || "$RUN" -eq 0 ]]; then
        bash tools/test-run-dashboard.sh "${DASH_ARGS[@]}"
        if [[ "$RUN" -eq 0 && "$READY" -eq 0 ]]; then
            bash tools/test-run-status-summary.sh --json
        fi
        if [[ "$RUN" -eq 0 ]]; then
            exit 0
        fi
    fi
fi

if [[ "$RUN" -eq 1 && "$NOPREFLIGHT" -eq 0 ]]; then
    if [[ "$JSON" -eq 1 ]]; then
        PREFLIGHT_JSON="$(bash tools/test-run-preflight.sh --json)"
        if [[ -z "$PREFLIGHT_JSON" || "${PREFLIGHT_JSON:0:1}" != "{" || "${PREFLIGHT_JSON: -1}" != "}" ]]; then
            echo "error: invalid preflight JSON payload" >&2
            exit 1
        fi
        PREPARED="$(python3 - "$PREFLIGHT_JSON" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
obj = payload.get("test_run_preflight", {}) if isinstance(payload, dict) else {}
ok = bool(obj.get("ready_for_test_run") and (obj.get("failed", 0) == 0))
print(0 if ok else 1)
PY
)"
        if [[ "$PREPARED" -ne 0 ]]; then
            echo "error: preflight not ready" >&2
            exit 1
        fi
        echo "$PREFLIGHT_JSON"
    else
        bash tools/test-run-preflight.sh || exit 1
        bash tools/test-run-status-summary.sh --blocker
    fi
fi

if [[ "$RUN" -eq 1 ]]; then
    STAGE="$(bash tools/current-stage.sh)"
    echo "[test-run-go] running stage ${STAGE}"
    if [[ "$JSON" -eq 1 ]]; then
        bash tools/test-run-stage.sh --json "$STAGE"
    else
        bash tools/test-run-stage.sh "$STAGE"
    fi
    exit 0
fi

bash tools/test-run-dashboard.sh
if [[ "$READY" -eq 1 ]]; then
    bash tools/test-run-preflight.sh
fi
