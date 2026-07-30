#!/usr/bin/env bash
set -euo pipefail

# Unified CI gate for test-run readiness: current-stage and next-stage.
# Usage:
#   bash tools/test-run-ci-gate.sh
#   bash tools/test-run-ci-gate.sh --json
#   bash tools/test-run-ci-gate.sh --skip-next
#   bash tools/test-run-ci-gate.sh --skip-current --json
#   bash tools/test-run-ci-gate.sh --by-ratio --top 5

JSON=0
CHECK_CURRENT=1
CHECK_NEXT=1
NEXT_ARGS=()

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json|--json-only)
            JSON=1
            shift
            ;;
        --skip-current)
            CHECK_CURRENT=0
            shift
            ;;
        --skip-next)
            CHECK_NEXT=0
            shift
            ;;
        -h|--help)
            sed -n '1,160p' "$0"
            exit 0
            ;;
        *)
            NEXT_ARGS+=("$1")
            shift
            ;;
    esac
done

CURRENT_JSON=''
NEXT_JSON=''
CURRENT_ERR=''
NEXT_ERR=''
CURRENT_RC=0
NEXT_RC=0

if [[ "$CHECK_CURRENT" -eq 1 ]]; then
    CURRENT_ERR_FILE="$(mktemp)"
    set +e
    CURRENT_JSON="$(bash tools/test-run-dashboard.sh --json 2>"$CURRENT_ERR_FILE")"
    CURRENT_RC=$?
    set -e
    CURRENT_ERR="$(cat "$CURRENT_ERR_FILE")"
    rm -f "$CURRENT_ERR_FILE"
else
    CURRENT_JSON='{}'
fi

if [[ "$CHECK_NEXT" -eq 1 ]]; then
    NEXT_ERR_FILE="$(mktemp)"
    set +e
    if [[ ${#NEXT_ARGS[@]} -gt 0 ]]; then
        NEXT_JSON="$(bash tools/test-run-go-next-ci.sh --json-only "${NEXT_ARGS[@]}" 2>"$NEXT_ERR_FILE")"
    else
        NEXT_JSON="$(bash tools/test-run-go-next-ci.sh --json-only 2>"$NEXT_ERR_FILE")"
    fi
    NEXT_RC=$?
    set -e
    NEXT_ERR="$(cat "$NEXT_ERR_FILE")"
    rm -f "$NEXT_ERR_FILE"
fi

if [[ "$JSON" -eq 1 ]]; then
    python3 - "$CURRENT_JSON" "$NEXT_JSON" "$CURRENT_ERR" "$NEXT_ERR" "$CHECK_CURRENT" "$CHECK_NEXT" "$CURRENT_RC" "$NEXT_RC" <<'PY'
import json
import sys

current_json = sys.argv[1]
next_json = sys.argv[2]
current_err = sys.argv[3].strip()
next_err = sys.argv[4].strip()
check_current = sys.argv[5] == "1"
check_next = sys.argv[6] == "1"
current_rc = int(sys.argv[7])
next_rc = int(sys.argv[8])

try:
    current_payload = json.loads(current_json) if current_json else {}
except Exception:
    current_payload = {}

try:
    next_payload = json.loads(next_json) if next_json else {}
except Exception:
    next_payload = {}

current_ready = False
if check_current:
    try:
        dashboard = current_payload.get("test_run_dashboard", {})
        signals = dashboard.get("signals", {})
        current_ready = bool(signals.get("can_run", False))
    except Exception:
        current_ready = False

next_ready = False
if check_next:
    next_ready = bool(next_payload.get("ci", {}).get("ready", False))

obj = {
    "ci": {
        "ready": bool(current_ready and next_ready) if check_current and check_next else bool(current_ready or next_ready),
        "checks": {
            "current": {
                "checked": check_current,
                "ready": current_ready,
                "error": current_err or None,
                "rc": current_rc,
            },
            "next": {
                "checked": check_next,
                "ready": next_ready,
                "error": next_err or None,
                "rc": next_rc,
            },
        },
    },
    "current": current_payload.get("test_run_dashboard") if isinstance(current_payload, dict) else {},
    "next": next_payload.get("payload") if isinstance(next_payload, dict) else {},
}

if check_current and not current_ready:
    obj["error"] = current_err or obj.get("error") or "current-stage readiness check failed"
elif check_next and not next_ready:
    obj["error"] = next_err or obj.get("error") or "next-stage readiness check failed"

print(json.dumps(obj, sort_keys=True))
PY

    if [[ "$CHECK_CURRENT" -eq 1 ]]; then
        if [[ "$CURRENT_RC" -ne 0 ]]; then
            exit "$CURRENT_RC"
        fi
    fi
    if [[ "$CHECK_NEXT" -eq 1 ]]; then
        if [[ "$NEXT_RC" -ne 0 ]]; then
            exit "$NEXT_RC"
        fi
    fi
    exit 0
fi

if [[ "$CHECK_CURRENT" -eq 1 ]]; then
    bash tools/test-run-dashboard.sh --assert-ready
fi

if [[ "$CHECK_NEXT" -eq 1 ]]; then
    if [[ ${#NEXT_ARGS[@]} -gt 0 ]]; then
        bash tools/test-run-go-next-ci.sh "${NEXT_ARGS[@]}"
    else
        bash tools/test-run-go-next-ci.sh
    fi
fi

exit 0
