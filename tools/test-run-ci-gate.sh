#!/usr/bin/env bash
set -euo pipefail

# Unified CI gate for test-run readiness: current-stage and next-stage.
# Usage:
#   bash tools/test-run-ci-gate.sh
#   bash tools/test-run-ci-gate.sh --json
#   bash tools/test-run-ci-gate.sh --skip-next
#   bash tools/test-run-ci-gate.sh --skip-current --json
#   bash tools/test-run-ci-gate.sh --by-ratio --top 5
#   bash tools/test-run-ci-gate.sh --run

JSON=0
CHECK_CURRENT=1
CHECK_NEXT=1
NEXT_ARGS=()
RUN_CURRENT=0
CURRENT_STAGE=""

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
        --run)
            RUN_CURRENT=1
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

CURRENT_STAGE="$(bash tools/current-stage.sh)"

CURRENT_JSON=''
NEXT_JSON=''
CURRENT_ERR=''
NEXT_ERR=''
RUN_ERR=''
RUN_RC=0
CURRENT_RC=0
NEXT_RC=0

require_json_obj() {
    local payload="$1"
    local parsed
    parsed="$(python3 - "$payload" <<'PY'
import json
import sys

payload = sys.argv[1]
try:
    obj = json.loads(payload)
except Exception:
    print("no")
    raise SystemExit

print("yes" if isinstance(obj, dict) else "no")
PY
)"
    if [[ "$parsed" != "yes" ]]; then
        return 1
    fi
}

extract_json_payload() {
    local output="$1"

    python3 - "$output" <<'PY'
import json
import sys

text = (sys.argv[1] or "").strip()

if not text:
    sys.exit(0)

try:
    payload = json.loads(text)
except Exception:
    payload = None

if payload is None:
    for line in reversed(text.splitlines()):
        candidate = line.strip()
        if not (candidate.startswith("{") and candidate.endswith("}")):
            continue
        try:
            payload = json.loads(candidate)
            break
        except Exception:
            continue

if isinstance(payload, dict):
    print(json.dumps(payload))
PY
}

compute_readiness() {
    python3 - "$CURRENT_JSON" "$NEXT_JSON" "$CHECK_CURRENT" "$CHECK_NEXT" "$CURRENT_RC" "$NEXT_RC" <<'PY'
import json
import sys

current_json = sys.argv[1]
next_json = sys.argv[2]
check_current = sys.argv[3] == "1"
check_next = sys.argv[4] == "1"
current_rc = int(sys.argv[5])
next_rc = int(sys.argv[6])

current_ready = False
next_ready = False

if check_current and current_rc == 0:
    try:
        current_payload = json.loads(current_json)
        dashboard = current_payload.get("test_run_dashboard", {})
        current_ready = bool(dashboard.get("signals", {}).get("can_run", False))
    except Exception:
        current_ready = False

if check_next and next_rc == 0:
    try:
        next_payload = json.loads(next_json)
        next_ready = bool(next_payload.get("ci", {}).get("ready", False))
    except Exception:
        next_ready = False

if check_current and check_next:
    ready = current_ready and next_ready
else:
    ready = current_ready or next_ready

print("1" if current_ready else "0")
print("1" if next_ready else "0")
print("1" if ready else "0")
PY
}

emit_ci_payload() {
    local run_requested="$1"
    local run_rc="$2"
    local current_ready="$3"
    local next_ready="$4"
    local current_err="$5"
    local next_err="$6"
    local run_err="$7"

    python3 - "$CURRENT_JSON" "$NEXT_JSON" "$CHECK_CURRENT" "$CHECK_NEXT" "$CURRENT_RC" "$NEXT_RC" "$run_requested" "$run_rc" "$current_ready" "$next_ready" "$current_err" "$next_err" "$run_err" <<'PY'
import json
import sys

current_json = sys.argv[1]
next_json = sys.argv[2]
check_current = sys.argv[3] == "1"
check_next = sys.argv[4] == "1"
current_rc = int(sys.argv[5])
next_rc = int(sys.argv[6])
run_requested = sys.argv[7] == "1"
run_rc = int(sys.argv[8])
current_ready = sys.argv[9] == "1"
next_ready = sys.argv[10] == "1"
current_err = sys.argv[11].strip()
next_err = sys.argv[12].strip()
run_err = sys.argv[13].strip()

try:
    current_payload = json.loads(current_json) if current_json else {}
except Exception:
    current_payload = {}

try:
    next_payload = json.loads(next_json) if next_json else {}
except Exception:
    next_payload = {}

if check_current and check_next:
    combined_ready = current_ready and next_ready
else:
    combined_ready = current_ready or next_ready

obj = {
    "ci": {
        "ready": bool(combined_ready),
        "checks": {
            "current": {
                "checked": check_current,
                "ready": current_ready,
                "error": current_err or None if check_current else None,
                "rc": current_rc,
            },
            "next": {
                "checked": check_next,
                "ready": next_ready,
                "error": next_err or None if check_next else None,
                "rc": next_rc,
            },
        },
    },
    "current": current_payload.get("test_run_dashboard") if isinstance(current_payload, dict) else {},
    "next": next_payload.get("payload") if isinstance(next_payload, dict) else {},
    "run": {
        "requested": run_requested,
        "rc": run_rc if run_requested else 0,
    },
}

if not combined_ready:
    if check_current and not current_ready:
        obj["error"] = current_err or "current-stage readiness check failed"
    elif check_next and not next_ready:
        obj["error"] = next_err or "next-stage readiness check failed"
    else:
        obj["error"] = "not ready to run current stage"
else:
    obj["error"] = run_err if run_requested and run_rc != 0 else obj.get("error")

if run_requested and run_rc != 0 and not obj.get("error"):
    obj["error"] = "stage run failed"

print(json.dumps(obj, sort_keys=True))
PY
}

if [[ "$CHECK_CURRENT" -eq 1 ]]; then
    set +e
    CURRENT_CAPTURE="$(bash tools/test-run-dashboard.sh --json 2>&1)"
    CURRENT_RC=$?
    set -e
    if [[ "$CURRENT_RC" -eq 0 ]]; then
        CURRENT_JSON="$(extract_json_payload "$CURRENT_CAPTURE")"
        if [[ -z "$CURRENT_JSON" ]] || ! require_json_obj "$CURRENT_JSON"; then
            CURRENT_RC=1
            CURRENT_ERR="invalid JSON payload from test-run-dashboard --json"
        else
            CURRENT_ERR=""
        fi
    else
        CURRENT_ERR="$CURRENT_CAPTURE"
        CURRENT_JSON=""
    fi
else
    CURRENT_JSON='{}'
fi

if [[ "$CHECK_NEXT" -eq 1 ]]; then
    set +e
    if [[ ${#NEXT_ARGS[@]} -gt 0 ]]; then
        NEXT_CAPTURE="$(bash tools/test-run-go-next-ci.sh --json-only "${NEXT_ARGS[@]}" 2>&1)"
    else
        NEXT_CAPTURE="$(bash tools/test-run-go-next-ci.sh --json-only 2>&1)"
    fi
    NEXT_RC=$?
    set -e
    if [[ "$NEXT_RC" -eq 0 ]]; then
        NEXT_JSON="$(extract_json_payload "$NEXT_CAPTURE")"
        if [[ -z "$NEXT_JSON" ]] || ! require_json_obj "$NEXT_JSON"; then
            NEXT_RC=1
            NEXT_ERR="invalid JSON payload from test-run-go-next-ci --json-only"
        else
            NEXT_ERR=""
        fi
    else
        NEXT_ERR="$NEXT_CAPTURE"
        NEXT_JSON=""
    fi
else
    NEXT_JSON='{}'
fi

read CURRENT_READY NEXT_READY COMBINED_READY <<<"$(compute_readiness)"

if [[ "$JSON" -eq 1 ]]; then
    if [[ "$RUN_CURRENT" -eq 1 ]]; then
        if [[ "$COMBINED_READY" != "1" ]]; then
            emit_ci_payload 0 0 "$CURRENT_READY" "$NEXT_READY" "$CURRENT_ERR" "$NEXT_ERR" ""
            exit 1
        fi

        set +e
        RUN_CAPTURE="$(TEST262_STAGE="$CURRENT_STAGE" bash tools/test-run-stage.sh 2>&1)"
        RUN_RC=$?
        set -e
        RUN_ERR="$RUN_CAPTURE"

        emit_ci_payload 1 "$RUN_RC" "$CURRENT_READY" "$NEXT_READY" "$CURRENT_ERR" "$NEXT_ERR" "$RUN_ERR"
        exit "$RUN_RC"
    fi

    emit_ci_payload 0 0 "$CURRENT_READY" "$NEXT_READY" "$CURRENT_ERR" "$NEXT_ERR" ""
    exit 0
fi

if [[ "$CHECK_CURRENT" -eq 1 && "$COMBINED_READY" != "1" ]]; then
    bash tools/test-run-dashboard.sh --assert-ready
fi

if [[ "$CHECK_NEXT" -eq 1 ]]; then
    if [[ ${#NEXT_ARGS[@]} -gt 0 ]]; then
        bash tools/test-run-go-next-ci.sh "${NEXT_ARGS[@]}"
    else
        bash tools/test-run-go-next-ci.sh
    fi
fi

if [[ "$RUN_CURRENT" -eq 1 ]]; then
    if [[ "$COMBINED_READY" != "1" ]]; then
        echo "error: readiness checks failed; aborting run" >&2
        exit 1
    fi
    TEST262_STAGE="$CURRENT_STAGE" bash tools/test-run-stage.sh "$CURRENT_STAGE"
fi

exit 0
