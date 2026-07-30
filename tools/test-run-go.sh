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
    bash tools/test-run-dashboard.sh --assert-ready || exit 1
    bash tools/test-run-preflight.sh || exit 1
    bash tools/test-run-status-summary.sh --blocker
fi

if [[ "$RUN" -eq 1 ]]; then
    STAGE="$(bash tools/current-stage.sh)"
    echo "[test-run-go] running stage ${STAGE}"
    bash tools/test-run-stage.sh "$STAGE"
    exit 0
fi

bash tools/test-run-dashboard.sh
if [[ "$READY" -eq 1 ]]; then
    bash tools/test-run-preflight.sh
fi
