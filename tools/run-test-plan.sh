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
#   bash tools/run-test-plan.sh --status --json --raw
#   bash tools/run-test-plan.sh --status --json --strict
#   bash tools/run-test-plan.sh --json --build

MODE="single"
JSON=0
STATUS=0
RAW=0
STRICT=0
DRY_RUN=1
BUILD=0
TOP=3
RATIO=0
STOP_ON_FAIL=0
MAX_FAILURES=0

run_json_status() {
    local mode="$1"
    local tmp
    local raw
    shift

    tmp="$(mktemp)"
    "$@" > "$tmp"
    raw="$(cat "$tmp")"
    rm -f "$tmp"

    python3 - "$mode" "$raw" "$STRICT" <<'PY'
import json
import sys

mode = sys.argv[1]
raw = sys.argv[2]
strict = sys.argv[3] == "1"

try:
    payload = json.loads(raw)
except json.JSONDecodeError:
    if strict:
        raise SystemExit("error: status output is not valid JSON")
    payload = {"raw": raw}

if strict:
    if not isinstance(payload, dict):
        raise SystemExit("error: status payload must be a JSON object")

    if mode == "single":
        if not isinstance(payload, dict):
            raise SystemExit("error: single status payload must be a JSON object")
        for key in ["source", "stage", "path"]:
            if key not in payload:
                raise SystemExit(f"error: missing field '{key}' in single status payload")
    elif mode == "batch":
        rp = payload
        if not isinstance(rp, dict) or "run-pending-batch" not in rp:
            raise SystemExit("error: missing 'run-pending-batch' in batch status payload")
        if "stages" not in rp.get("run-pending-batch", {}):
            raise SystemExit("error: missing 'stages' in run-pending-batch status payload")
    else:
        raise SystemExit("error: unknown run-test-plan mode")

print(json.dumps({
    "run-test-plan": {
        "mode": mode,
        "status_payload": payload,
    }
}, sort_keys=True))
PY
}

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
        --raw)
            RAW=1
            shift
            ;;
        --strict)
            STRICT=1
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
            sed -n '1,180p' "$0"
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
        [[ "$JSON" -eq 1 ]] && FLAGS+=(--json)
        if [[ "$JSON" -eq 1 && "$RAW" -eq 0 ]]; then
            run_json_status "batch" bash tools/run-pending-batch.sh "${FLAGS[@]}"
        else
            bash tools/run-pending-batch.sh "${FLAGS[@]}"
        fi
        exit 0
    fi

    if [[ "$DRY_RUN" -eq 1 ]]; then
        if [[ "$JSON" -eq 1 && "$RAW" -eq 0 ]]; then
            run_json_status "batch" bash tools/run-pending-batch.sh "${FLAGS[@]}" --status
            exit 0
        fi
        bash tools/run-pending-batch.sh "${FLAGS[@]}"
        exit 0
    fi

    FLAGS+=(--run)
    if [[ "$JSON" -eq 1 ]]; then
        FLAGS+=(--status --json)
        if [[ "$RAW" -eq 0 ]]; then
            run_json_status "batch" bash tools/run-pending-batch.sh "${FLAGS[@]}"
            exit 0
        fi
        bash tools/run-pending-batch.sh "${FLAGS[@]}"
        exit 0
    fi

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
        if [[ "$RAW" -eq 0 ]]; then
            run_json_status "single" bash tools/run-next-pending.sh "${FLAGS[@]}"
            exit 0
        fi
        bash tools/run-next-pending.sh "${FLAGS[@]}"
        exit 0
    fi
    FLAGS+=(--print)
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
