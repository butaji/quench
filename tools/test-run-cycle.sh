#!/usr/bin/env bash
set -euo pipefail

# SSOT == test-run end-to-end cycle helper:
# preflight, dry-gate, optional run, and optional milestone commit/push.
# Usage:
#   bash tools/test-run-cycle.sh
#   bash tools/test-run-cycle.sh --run
#   bash tools/test-run-cycle.sh --run --assert-ready
#   bash tools/test-run-cycle.sh --run --json
#   bash tools/test-run-cycle.sh --run --commit
#   bash tools/test-run-cycle.sh --run --commit --push
#   bash tools/test-run-cycle.sh --run --commit "chore: stage progress"

RUN=0
ASSERT_READY=0
JSON=0
DASHBOARD_PRECHECK_DONE=0
AUTO_COMMIT=0
AUTO_PUSH=0
COMMIT_MESSAGE=""

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --run)
            RUN=1
            shift
            ;;
        --assert-ready)
            ASSERT_READY=1
            shift
            ;;
        --json)
            JSON=1
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
            sed -n '1,140p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ "$JSON" -eq 1 ]]; then
    DASHBOARD_ARGS=(--json)
    if [[ "$ASSERT_READY" -eq 1 ]]; then
        DASHBOARD_ARGS+=(--assert-ready)
    fi
    if ! DASHBOARD_JSON=$(bash tools/test-run-dashboard.sh "${DASHBOARD_ARGS[@]}"); then
        echo "error: dashboard check failed" >&2
        exit 1
    fi
    DASHBOARD_PRECHECK_DONE=1
    if [[ "$RUN" -eq 0 ]]; then
        echo "$DASHBOARD_JSON"
        exit 0
    fi
    echo "$DASHBOARD_JSON"
fi

if [[ "$ASSERT_READY" -eq 1 && "$DASHBOARD_PRECHECK_DONE" -eq 0 ]]; then
    bash tools/test-run-dashboard.sh --assert-ready || exit 1
fi

if [[ "$RUN" -eq 1 ]]; then
    if [[ "$AUTO_COMMIT" -eq 1 && "$JSON" -eq 1 ]]; then
        echo "error: --json is not supported with --commit in this command" >&2
        exit 1
    fi
    if ! bash tools/test-run-preflight.sh ${JSON:+--json}; then
        echo "error: preflight check failed" >&2
        exit 1
    fi
    bash tools/test-run-status-summary.sh --blocker
    if [[ "$AUTO_COMMIT" -eq 1 ]]; then
        STAGE_ARGS=(--test-run)
        if [[ -n "$COMMIT_MESSAGE" ]]; then
            STAGE_ARGS+=(--commit "$COMMIT_MESSAGE")
        else
            STAGE_ARGS+=(--commit)
        fi
        if [[ "$AUTO_PUSH" -eq 1 ]]; then
            STAGE_ARGS+=(--push)
        fi
        bash tools/milestone.sh "${STAGE_ARGS[@]}"
        exit 0
    fi
    STAGE="$(bash tools/current-stage.sh)"
    echo "[test-run-cycle] running test-run for stage ${STAGE}"
    bash tools/test-run-stage.sh "$STAGE"
    exit 0
fi

bash tools/test-run-dashboard.sh
