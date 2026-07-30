#!/usr/bin/env bash
set -euo pipefail

# End-to-end cycle helper for a stage test-run:
# dry-gate then optional run + summary.
# Usage:
#   bash tools/test-run-cycle.sh
#   bash tools/test-run-cycle.sh --run
#   bash tools/test-run-cycle.sh --run --assert-ready
#   bash tools/test-run-cycle.sh --run --json

RUN=0
ASSERT_READY=0
JSON=0

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
    if [[ "$RUN" -eq 0 ]]; then
        echo "$DASHBOARD_JSON"
        exit 0
    fi
    echo "$DASHBOARD_JSON"
fi

if [[ "$ASSERT_READY" -eq 1 ]]; then
    bash tools/test-run-dashboard.sh --assert-ready || exit 1
fi

if [[ "$RUN" -eq 1 ]]; then
    bash tools/test-run-status-summary.sh --blocker
    STAGE="$(bash tools/current-stage.sh)"
    echo "[test-run-cycle] running test-run for stage ${STAGE}"
    bash tools/test-run-stage.sh "$STAGE"
    exit 0
fi

bash tools/test-run-dashboard.sh
