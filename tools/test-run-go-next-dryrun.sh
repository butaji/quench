#!/usr/bin/env bash
set -euo pipefail

# Dry-run validator for next-stage flow (no stage execution).
# Usage:
#   bash tools/test-run-go-next-dryrun.sh
#   bash tools/test-run-go-next-dryrun.sh --print-json
#   bash tools/test-run-go-next-dryrun.sh --run-check
#   bash tools/test-run-go-next-dryrun.sh --assert-ready
#   bash tools/test-run-go-next-dryrun.sh --by-ratio --top 5

PRINT_JSON=0
RUN_CHECK=0
ASSERT_READY=0
ARGS=()

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --print-json)
            PRINT_JSON=1
            shift
            ;;
        --run-check)
            RUN_CHECK=1
            shift
            ;;
        --assert-ready)
            ASSERT_READY=1
            RUN_CHECK=1
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

if [[ ${#ARGS[@]} -eq 0 ]]; then
    PAYLOAD="$(bash tools/test-run-go-next.sh --print-json)"
else
    PAYLOAD="$(bash tools/test-run-go-next.sh --print-json "${ARGS[@]}")"
fi

if [[ "$PRINT_JSON" -eq 1 ]]; then
    printf '%s
' "$PAYLOAD"
else
    read -r STAGE STAGE_PATH SOURCE CURRENT MATCH AUTO <<<"$(python3 - "$PAYLOAD" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
obj = payload.get("test_run_go_next", {})
print(
    obj.get("stage", ""),
    obj.get("path", ""),
    obj.get("source", ""),
    obj.get("current_stage", ""),
    obj.get("matches_current", False),
    obj.get("can_auto_advance", False),
)
PY
)"
    echo "stage=$STAGE"
    echo "path=$STAGE_PATH"
    echo "source=$SOURCE"
    echo "current_stage=$CURRENT"
    echo "matches_current=$MATCH"
    echo "can_auto_advance=$AUTO"
fi

MATCHES_CURRENT="$(python3 - "$PAYLOAD" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
obj = payload.get("test_run_go_next", {})
print("1" if obj.get("matches_current") else "0")
PY
)"

if [[ "$RUN_CHECK" -eq 1 ]]; then
    if [[ "$MATCHES_CURRENT" == "1" ]]; then
        PREFLIGHT_JSON="$(bash tools/test-run-preflight.sh --json)"
        if [[ "$(python3 - "$PREFLIGHT_JSON" <<'PY'
import json
import sys

try:
    json.loads(sys.argv[1])
    print(0)
except Exception:
    print(1)
PY
)" -eq 1 ]]; then
            exit 1
        fi
        if [[ "$ASSERT_READY" -eq 1 ]]; then
            if [[ "$(python3 - "$PREFLIGHT_JSON" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
obj = payload.get("test_run_go_next", {})
print(0 if obj.get("can_auto_advance") else 1)
PY
)" -eq 1 ]]; then
                echo "[test-run-go-next-dryrun] stage is not auto-advance ready in preflight payload" >&2
                exit 1
            fi
        fi
    else
        echo "[test-run-go-next-dryrun] skipped preflight check (target is not current stage)" >&2
        if [[ "$ASSERT_READY" -eq 1 ]]; then
            echo "[test-run-go-next-dryrun] assert-ready failed: matches_current=false" >&2
            exit 1
        fi
    fi
fi

exit 0
