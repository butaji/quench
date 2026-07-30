#!/usr/bin/env bash
set -euo pipefail

# One-command helper for the next pending stage test-run.
# Usage:
#   bash tools/test-run-go-next.sh
#   bash tools/test-run-go-next.sh --run
#   bash tools/test-run-go-next.sh --run --json
#   bash tools/test-run-go-next.sh --json
#   bash tools/test-run-go-next.sh --print
#   bash tools/test-run-go-next.sh --print-json
#   bash tools/test-run-go-next.sh --status
#   bash tools/test-run-go-next.sh --run --by-ratio --top 5

RUN=0
JSON=0
PRINT_ONLY=0
PRINT_JSON=0
BY_RATIO=0
TOP=1
NOPREFLIGHT=0
STAGE_OVERRIDE=""

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --run)
            RUN=1
            shift
            ;;
        --json)
            JSON=1
            shift
            ;;
        --print)
            PRINT_ONLY=1
            shift
            ;;
        --print-json|--status)
            PRINT_ONLY=1
            PRINT_JSON=1
            JSON=1
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
        --no-preflight)
            NOPREFLIGHT=1
            shift
            ;;
        --stage)
            if [[ "${2:-}" == "" || ! "${2:-}" =~ ^[0-9]+$ ]]; then
                echo "error: --stage requires a numeric argument" >&2
                exit 1
            fi
            STAGE_OVERRIDE="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '1,180p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -n "$STAGE_OVERRIDE" ]]; then
    STAGE="$STAGE_OVERRIDE"
    SOURCE="override"
else
    if [[ "$BY_RATIO" -eq 1 ]]; then
        SOURCE="ratio"
        RATIO_PAYLOAD="$(mktemp)"
        bash tools/pending-stages.sh --top-ratio "$TOP" --json > "$RATIO_PAYLOAD"
        STAGE="$(python3 - "$RATIO_PAYLOAD" <<'PY'
import json
import sys

try:
    with open(sys.argv[1]) as f:
        data = json.load(f)
except (OSError, json.JSONDecodeError) as exc:
    print(f"error: failed to read ratio payload: {exc}", file=sys.stderr)
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
        SOURCE="next"
        STAGE="$(bash tools/next-stage.sh)"
    fi
fi

if [[ -z "$STAGE" || "$STAGE" == "0" ]]; then
    echo "No pending stage found." >&2
    exit 1
fi

STAGE_PATH="$(bash tools/stage-path.sh "$STAGE")"

if [[ "$JSON" -eq 1 ]]; then
    PAYLOAD="$(python3 - "$STAGE" "$SOURCE" "$STAGE_PATH" <<'PY'
import json
import sys

stage = int(sys.argv[1])
source = sys.argv[2]
path = sys.argv[3]

payload = {
    "test_run_go_next": {
        "source": source,
        "stage": stage,
        "path": path,
    }
}
print(json.dumps(payload, sort_keys=True))
PY
)"
fi

if [[ "$PRINT_ONLY" -eq 1 ]]; then
    if [[ "$PRINT_JSON" -eq 1 ]]; then
        printf '%s\n' "$PAYLOAD"
    else
        echo "$STAGE"
    fi
    exit 0
fi

if [[ "$RUN" -eq 0 ]]; then
    if [[ "$JSON" -eq 1 ]]; then
        printf '%s\n' "$PAYLOAD"
        exit 0
    fi
    echo "stage=${STAGE}"
    echo "path=${STAGE_PATH}"
    echo "source=${SOURCE}"
    exit 0
fi

if [[ "$NOPREFLIGHT" -eq 0 ]]; then
    CURRENT="$(bash tools/current-stage.sh)"
    if [[ "$STAGE" == "$CURRENT" ]]; then
        bash tools/test-run-preflight.sh || exit 1
    else
        echo "[test-run-go-next] preflight skipped (target ${STAGE} != current ${CURRENT})" >&2
    fi
fi

echo "[test-run-go-next] running stage ${STAGE} (${SOURCE})"
if [[ "$JSON" -eq 1 ]]; then
    TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --json --
else
    TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --
fi
