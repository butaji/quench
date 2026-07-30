#!/usr/bin/env bash
set -euo pipefail

# Run a test-plan selection (single next stage or batch) in one command.
# Usage:
#   bash tools/run-test-plan.sh
#   bash tools/run-test-plan.sh --batch
#   bash tools/run-test-plan.sh --batch --ratio --top 5
#   bash tools/run-test-plan.sh --batch --run
#   bash tools/run-test-plan.sh --batch --run --status
#   bash tools/run-test-plan.sh --batch --run --json
#   bash tools/run-test-plan.sh --status --json
#   bash tools/run-test-plan.sh --json --build

MODE="single"
JSON=0
STATUS=0
DRY_RUN=1
BUILD=0
TOP=3
RATIO=0
STOP_ON_FAIL=0
MAX_FAILURES=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --single)
            MODE="single"
            shift
            ;;
        --batch)
            MODE="batch"
            shift
            ;;
        --json)
            JSON=1
            shift
            ;;
        --status)
            STATUS=1
            shift
            ;;
        --run)
            DRY_RUN=0
            shift
            ;;
        --build)
            BUILD=1
            shift
            ;;
        --ratio)
            RATIO=1
            shift
            ;;
        --top)
            if [[ "${2:-}" == "" || ! "${2:-}" =~ ^[0-9]+$ ]]; then
                echo "error: --top requires a numeric argument" >&2
                exit 1
            fi
            TOP="$2"
            shift 2
            ;;
        --stop-on-fail)
            STOP_ON_FAIL=1
            shift
            ;;
        --max-failures)
            if [[ "${2:-}" == "" || ! "${2:-}" =~ ^[0-9]+$ ]]; then
                echo "error: --max-failures requires a numeric argument" >&2
                exit 1
            fi
            MAX_FAILURES="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '1,160p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
 done

if [[ "$MODE" == "batch" ]]; then
    FLAGS=(--top "$TOP")
    [[ "$RATIO" -eq 1 ]] && FLAGS+=(--ratio)
    [[ "$BUILD" -eq 1 ]] && FLAGS+=(--build)
    [[ "$STOP_ON_FAIL" -eq 1 ]] && FLAGS+=(--stop-on-fail)
    [[ "$MAX_FAILURES" -gt 0 ]] && FLAGS+=(--max-failures "$MAX_FAILURES")

    if [[ "$STATUS" -eq 1 ]]; then
        FLAGS+=(--status)
        [[ "$DRY_RUN" -eq 0 ]] && FLAGS+=(--run)
        if [[ "$JSON" -eq 1 ]]; then
            FLAGS+=(--json)
        fi
        bash tools/run-pending-batch.sh "${FLAGS[@]}"
        exit 0
    fi

    if [[ "$DRY_RUN" -eq 1 ]]; then
        bash tools/run-pending-batch.sh "${FLAGS[@]}"
        exit 0
    fi

    if [[ "$JSON" -eq 1 ]]; then
        FLAGS+=(--status --json)
    fi

    FLAGS+=(--run)
    bash tools/run-pending-batch.sh "${FLAGS[@]}"
    exit 0
fi

FLAGS=()
if [[ "$RATIO" -eq 1 ]]; then
    FLAGS+=(--by-ratio --top "$TOP")
fi
if [[ "$BUILD" -eq 1 ]]; then
    FLAGS+=(--build)
fi

if [[ "$STATUS" -eq 1 ]]; then
    if [[ "$JSON" -eq 1 ]]; then
        FLAGS+=(--status)
    else
        FLAGS+=(--print)
    fi
    bash tools/run-next-pending.sh "${FLAGS[@]}"
    exit 0
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
    if [[ "$JSON" -eq 1 ]]; then
        bash tools/run-next-pending.sh --print-json "${FLAGS[@]}"
    else
        bash tools/run-next-pending.sh --print "${FLAGS[@]}"
    fi
    exit 0
fi

if [[ "$JSON" -eq 1 ]]; then
    bash tools/run-next-pending.sh --json "${FLAGS[@]}"
    exit 0
fi

bash tools/run-next-pending.sh "${FLAGS[@]}"
