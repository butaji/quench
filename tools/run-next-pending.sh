#!/usr/bin/env bash
set -euo pipefail

# Run the next pending stage in one command.
# Usage:
#   bash tools/run-next-pending.sh
#   bash tools/run-next-pending.sh --stage-override <n>
#   bash tools/run-next-pending.sh --json --build
#   bash tools/run-next-pending.sh --by-ratio --top 3

RUN_JSON=0
RUN_BUILD=0
BY_RATIO=0
TOP=1
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
        --by-ratio)
            BY_RATIO=1
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
    if [[ "$BY_RATIO" -eq 1 ]]; then
        RATIO_PAYLOAD="$(mktemp)"
        bash tools/pending-stages.sh --top-ratio "$TOP" --json > "$RATIO_PAYLOAD"
        STAGE="$(python3 - "$RATIO_PAYLOAD" <<'PY'
import json
import sys

try:
    with open(sys.argv[1]) as f:
        data = json.load(f)
except (OSError, json.JSONDecodeError) as exc:
    print(f"error: failed to read next ratio stage payload: {exc}", file=sys.stderr)
    raise SystemExit(1)

stages = data.get('stages', [])
if not stages:
    print(0)
    raise SystemExit(0)
print(stages[0].get('id', 0))
PY
)"
        rm -f "$RATIO_PAYLOAD"
    else
        STAGE="$(bash tools/next-stage.sh)"
    fi
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
