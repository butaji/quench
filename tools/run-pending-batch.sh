#!/usr/bin/env bash
set -euo pipefail

# Run or print a batch of pending stages.
# Usage:
#   bash tools/run-pending-batch.sh                    # show top 3 pending by fail count
#   bash tools/run-pending-batch.sh --top 5 --ratio
#   bash tools/run-pending-batch.sh --top 3 --run

TOP=3
RATIO=0
BUILD=0
DRY_RUN=1
JSON=0

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

PAYLOAD="$("${PAYLOAD_CMD[@]}")"
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
while IFS= read -r STAGE; do
    [[ -z "$STAGE" ]] && continue
    echo "[run-pending-batch] stage=${STAGE}"
    if ! TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --; then
        echo "[run-pending-batch] stage ${STAGE} failed" >&2
        FAILED=1
    fi

done <<< "$STAGES"

if [[ "$FAILED" -ne 0 ]]; then
    exit 1
fi
