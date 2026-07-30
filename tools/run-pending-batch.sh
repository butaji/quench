#!/usr/bin/env bash
set -euo pipefail

# Run or print a batch of pending stages.
# Usage:
#   bash tools/run-pending-batch.sh                    # show top 3 pending by fail count
#   bash tools/run-pending-batch.sh --top 5 --ratio
#   bash tools/run-pending-batch.sh --top 3 --run
#   bash tools/run-pending-batch.sh --run --stop-on-fail
#   bash tools/run-pending-batch.sh --run --max-failures 2

TOP=3
RATIO=0
BUILD=0
DRY_RUN=1
JSON=0
STATUS=0
STOP_ON_FAIL=0
MAX_FAILURES=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --top)
            if [[ "${2:-}" == "" || ! "${2:-}" =~ ^[0-9]+$ ]]; then
                echo "error: --top requires a numeric argument" >&2
                exit 1
            fi
            TOP="$2"
            shift 2
            ;;
        --ratio)
            RATIO=1
            shift
            ;;
        --build)
            BUILD=1
            shift
            ;;
        --run)
            DRY_RUN=0
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
            sed -n '1,120p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

cd "$(dirname "$0")/.."

if [[ "$RATIO" -eq 1 ]]; then
    PAYLOAD_CMD=(bash tools/pending-stages.sh --top-ratio "$TOP" --json)
else
    PAYLOAD_CMD=(bash tools/pending-stages.sh --top "$TOP" --json)
fi

if [[ "$BUILD" -eq 1 ]]; then
    export TEST262_TEST_RUN_BUILD=1
fi

PAYLOAD="$(${PAYLOAD_CMD[@]})"
STAGES=$(python3 - "$PAYLOAD" <<'PY'
import json
import sys

data = json.loads(sys.argv[1])
print("\n".join(str(s.get('id', '')) for s in data.get('stages', [])))
PY
)

if [[ -z "$STAGES" ]]; then
    echo "No pending stages found for requested batch."
    exit 0
fi

if [[ "$JSON" -eq 1 ]]; then
    echo "$PAYLOAD"
    exit 0
fi

if [[ "$STATUS" -eq 1 ]]; then
    export TOP_BATCH="$TOP"
    export RATIO_BATCH="$RATIO"
    export DRY_RUN_BATCH="$DRY_RUN"
    export BUILD_BATCH="$BUILD"
    export STOP_ON_FAIL_BATCH="$STOP_ON_FAIL"
    export MAX_FAILURES_BATCH="$MAX_FAILURES"
    export PAYLOAD_BATCH="$PAYLOAD"
    python3 - <<PY
import json
import os

data = json.loads(os.environ["PAYLOAD_BATCH"])
payload = {
    "count": data.get("count", 0),
    "top": int(os.environ["TOP_BATCH"]),
    "ratio": os.environ["RATIO_BATCH"] == "1",
    "run": os.environ["DRY_RUN_BATCH"] == "0",
    "build": os.environ["BUILD_BATCH"] == "1",
    "stop_on_fail": os.environ["STOP_ON_FAIL_BATCH"] == "1",
    "max_failures": int(os.environ["MAX_FAILURES_BATCH"]),
    "stages": data.get("stages", []),
}
print(json.dumps({"run-pending-batch": payload}, sort_keys=True))
PY
    exit 0
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Planned batch (top ${TOP}, ratio=${RATIO}):"
    while IFS= read -r S; do
        [[ -z "$S" ]] && continue
        P="$(bash tools/stage-path.sh "$S")"
        printf '  - stage=%s path=%s\n' "$S" "$P"
    done <<< "$STAGES"
    exit 0
fi

FAILED=0
FAILED_STAGES=()
FAIL_COUNT=0
while IFS= read -r STAGE; do
    [[ -z "$STAGE" ]] && continue
    echo "[run-pending-batch] stage=${STAGE}"
    if ! TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --; then
        echo "[run-pending-batch] stage ${STAGE} failed" >&2
        FAILED=1
        FAIL_COUNT=$((FAIL_COUNT + 1))
        FAILED_STAGES+=("${STAGE}")

        if [[ "$STOP_ON_FAIL" -eq 1 ]]; then
            echo "[run-pending-batch] stop-on-fail requested; aborting batch" >&2
            break
        fi

        if [[ "$MAX_FAILURES" -gt 0 && "$FAIL_COUNT" -ge "$MAX_FAILURES" ]]; then
            echo "[run-pending-batch] max-failures (${MAX_FAILURES}) reached; aborting batch" >&2
            break
        fi
    fi
done <<< "$STAGES"

if [[ "$FAILED" -ne 0 ]]; then
    printf '[run-pending-batch] failed=%s failed_stages=%s\n' "$FAIL_COUNT" "${FAILED_STAGES[*]}" >&2
    if [[ "$MAX_FAILURES" -gt 0 ]]; then
        echo "[run-pending-batch] max-failure budget remaining: $((MAX_FAILURES - FAIL_COUNT))" >&2
    fi
    exit 1
fi
