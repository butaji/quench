#!/usr/bin/env bash
set -euo pipefail

# Run the next pending stage in one command.
# Usage:
#   bash tools/run-next-pending.sh
#   bash tools/run-next-pending.sh --stage-override <n>
#   bash tools/run-next-pending.sh --json --build

RUN_JSON=0
RUN_BUILD=0
STAGE_OVERRIDE=""

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            RUN_JSON=1
            shift
            ;;
        --build)
            RUN_BUILD=1
            shift
            ;;
        --stage-override)
            if [[ "${2:-}" == "" || ! "${2:-}" =~ ^[0-9]+$ ]]; then
                echo "error: --stage-override requires a numeric stage id" >&2
                exit 1
            fi
            STAGE_OVERRIDE="$2"
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

cd "$(dirname "$0")/.."

if [[ -n "$STAGE_OVERRIDE" ]]; then
    STAGE="$STAGE_OVERRIDE"
else
    STAGE="$(bash tools/next-stage.sh)"
fi

if [[ -z "${STAGE}" || "${STAGE}" == "0" ]]; then
    echo "No pending stage found." >&2
    exit 1
fi

if [[ "$RUN_BUILD" -eq 1 ]]; then
    export TEST262_TEST_RUN_BUILD=1
fi

if [[ "$RUN_JSON" -eq 1 ]]; then
    TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --json --
    exit $?
fi

echo "[run-next-pending] Stage ${STAGE}"
TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --
