#!/usr/bin/env bash
set -euo pipefail

# One-step milestone flow.
# Usage:
#   bash tools/ship-milestone.sh                    # run next stage, commit
#   bash tools/ship-milestone.sh --push              # run next stage, commit, push
#   bash tools/ship-milestone.sh --message "..."      # custom commit message
#   bash tools/ship-milestone.sh --unit "test-filter" # run unit test filter before stage run
#   bash tools/ship-milestone.sh --json               # emit machine readable output

PUSH=0
JSON=0
MESSAGE=""
UNIT_FILTER=""

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --push)
            PUSH=1
            shift
            ;;
        --json)
            JSON=1
            shift
            ;;
        --unit)
            if [[ "${2:-}" == "" || "${2:-}" == --* ]]; then
                echo "error: --unit requires a test name filter argument" >&2
                exit 1
            fi
            UNIT_FILTER="$2"
            shift 2
            ;;
        --message)
            if [[ "${2:-}" == "" || "${2:-}" == --* ]]; then
                echo "error: --message requires a text argument" >&2
                exit 1
            fi
            MESSAGE="$2"
            shift 2
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

RUN_ARGS=(--run --advance)
if [[ "$JSON" -eq 1 ]]; then
    RUN_ARGS+=(--json)
fi
if [[ -n "$MESSAGE" ]]; then
    RUN_ARGS+=(--commit "$MESSAGE")
else
    RUN_ARGS+=(--commit)
fi
if [[ "$PUSH" -eq 1 ]]; then
    RUN_ARGS+=(--push)
fi

if [[ -n "$UNIT_FILTER" ]]; then
    cargo test -p quench-runtime "$UNIT_FILTER" -- --exact
    echo "[ship-milestone] unit preflight passed: $UNIT_FILTER"
    echo
fi

if [[ "$JSON" -eq 1 ]]; then
    bash tools/test-run-go-next.sh "${RUN_ARGS[@]}"
    exit 0
fi

bash tools/test-run-status-summary.sh
bash tools/test-run-go-next.sh "${RUN_ARGS[@]}"
