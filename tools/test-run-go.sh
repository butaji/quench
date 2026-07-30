#!/usr/bin/env bash
set -euo pipefail

# Single command to validate readiness and run a stage test-run.
# Usage:
#   bash tools/test-run-go.sh
#   bash tools/test-run-go.sh --ready
#   bash tools/test-run-go.sh --run
#   bash tools/test-run-go.sh --run --json
#   bash tools/test-run-go.sh --run --no-preflight
#   bash tools/test-run-go.sh --run --commit
#   bash tools/test-run-go.sh --run --commit "chore: stage 34 progress"
#   bash tools/test-run-go.sh --run --commit --push
#   `test-run` is the canonical term for milestone stage runs.

JSON=0
RUN=0
READY=0
NOPREFLIGHT=0
DASHBOARD_DONE=0
CURRENT_STAGE=""
AUTO_COMMIT=0
AUTO_PUSH=0
COMMIT_MESSAGE=""

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
        --commit)
            AUTO_COMMIT=1
            if [[ "${2:-}" != "" && "${2:-}" != --* ]]; then
                COMMIT_MESSAGE="$2"
                shift 2
            else
                shift
            fi
            ;;
        --push)
            AUTO_PUSH=1
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

if [[ "$RUN" -eq 1 ]]; then
    CURRENT_STAGE="$(bash tools/current-stage.sh)"
fi

if [[ "$JSON" -eq 1 ]]; then
    DASH_ARGS=(--json)
    if [[ "$READY" -eq 1 || "$RUN" -eq 0 ]]; then
        bash tools/test-run-dashboard.sh "${DASH_ARGS[@]}"
        DASHBOARD_DONE=1
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
if not obj.get("ready_for_test_run"):
    print(1)
    sys.exit()
failed = int(obj.get("failed", 0) or 0)
if failed > 0:
    print(2)
    sys.exit()
print(0)
PY
)"
        if [[ "$PREPARED" == "1" ]]; then
            echo "error: preflight not ready" >&2
            exit 1
        elif [[ "$PREPARED" == "2" ]]; then
            echo "warning: preflight reports prior failures; proceeding" >&2
            PREPARED=0
        elif [[ "$PREPARED" != "0" ]]; then
            echo "error: preflight parser failed" >&2
            exit 1
        fi
        echo "$PREFLIGHT_JSON"
    else
        PREFLIGHT_JSON="$(bash tools/test-run-preflight.sh --json)"
        if [[ -z "$PREFLIGHT_JSON" || "${PREFLIGHT_JSON:0:1}" != "{" || "${PREFLIGHT_JSON: -1}" != "}" ]]; then
            echo "error: invalid preflight JSON payload" >&2
            exit 1
        fi
        python3 - "$PREFLIGHT_JSON" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
obj = payload.get("test_run_preflight", {}) if isinstance(payload, dict) else {}
if not obj.get("ready_for_test_run"):
    print("error: preflight not ready", file=sys.stderr)
    raise SystemExit(1)
failed = int(obj.get("failed", 0) or 0)
if failed > 0:
    print(f"warning: preflight reports {failed} previous failures; proceeding", file=sys.stderr)
PY
        bash tools/test-run-status-summary.sh
    fi
fi

if [[ "$RUN" -eq 1 ]]; then
    if [[ "$JSON" -eq 1 && "$AUTO_COMMIT" -eq 1 ]]; then
        echo "error: --json is not supported with --commit in this command" >&2
        exit 1
    fi
    if [[ "$AUTO_COMMIT" -eq 1 ]]; then
        MILESTONE_ARGS=(--stage "$CURRENT_STAGE" --test-run)
        if [[ "$AUTO_PUSH" -eq 1 ]]; then
            MILESTONE_ARGS+=(--push)
        fi
        if [[ -n "$COMMIT_MESSAGE" ]]; then
            MILESTONE_ARGS+=(--commit "$COMMIT_MESSAGE")
        else
            MILESTONE_ARGS+=(--commit)
        fi
        bash tools/milestone.sh "${MILESTONE_ARGS[@]}"
        RUN_RC=$?
    else
        echo "[test-run-go] running stage ${CURRENT_STAGE}"
        if [[ "$JSON" -eq 1 ]]; then
            bash tools/test-run-stage.sh --json "$CURRENT_STAGE"
        else
            bash tools/test-run-stage.sh "$CURRENT_STAGE"
        fi
        RUN_RC=$?
    fi
    if [[ "$RUN_RC" -ne 0 ]]; then
        exit "$RUN_RC"
    fi
    exit 0
fi

if [[ "$DASHBOARD_DONE" -eq 0 ]]; then
    bash tools/test-run-dashboard.sh
fi
if [[ "$READY" -eq 1 ]]; then
    bash tools/test-run-preflight.sh
fi
