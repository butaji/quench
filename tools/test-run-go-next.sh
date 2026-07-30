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
#   bash tools/test-run-go-next.sh --run --advance
#   bash tools/test-run-go-next.sh --run --commit
#   bash tools/test-run-go-next.sh --run --commit "chore: stage fix"
#   bash tools/test-run-go-next.sh --run --commit --push

RUN=0
JSON=0
PRINT_ONLY=0
PRINT_JSON=0
BY_RATIO=0
TOP=1
NOPREFLIGHT=0
AUTO_ADVANCE=0
AUTO_COMMIT=0
AUTO_PUSH=0
COMMIT_MESSAGE=""
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
        --advance)
            AUTO_ADVANCE=1
            shift
            ;;
        --commit)
            AUTO_COMMIT=1
            if [[ "${2:-}" != "" && "${2:-}" != --* ]]; then
                COMMIT_MESSAGE="$2"
                shift 2
            else
                shift
            fi
            ;;
        --push)
            AUTO_PUSH=1
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
            sed -n '1,200p' "$0"
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
        RATIO_OUTPUT="$(bash tools/pending-stages.sh --top-ratio "$TOP" --json)"
        RATIO_RC=$?

        if [[ "$RATIO_RC" -ne 0 ]]; then
            echo "$RATIO_OUTPUT" >&2
            exit "$RATIO_RC"
        fi

        STAGE="$(python3 - "$RATIO_OUTPUT" <<'PY'
import json
import sys

try:
    data = json.loads(sys.argv[1])
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
CURRENT_STAGE="$(bash tools/current-stage.sh)"
CAN_AUTO_ADVANCE="false"
if [[ "$STAGE" == "$CURRENT_STAGE" ]]; then
    CAN_AUTO_ADVANCE="true"
fi

if [[ "$JSON" -eq 1 ]]; then
    PAYLOAD="$(python3 - "$STAGE" "$SOURCE" "$STAGE_PATH" "$CURRENT_STAGE" "$CAN_AUTO_ADVANCE" <<'PY'
import json
import sys

stage = int(sys.argv[1])
source = sys.argv[2]
path = sys.argv[3]
current = int(sys.argv[4])
can_auto_advance = bool(sys.argv[5].lower() == "true")

payload = {
    "test_run_go_next": {
        "source": source,
        "stage": stage,
        "path": path,
        "current_stage": current,
        "matches_current": stage == current,
        "can_auto_advance": can_auto_advance,
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
    if [[ "$STAGE" == "$CURRENT_STAGE" ]]; then
        bash tools/test-run-preflight.sh || exit 1
    else
        echo "[test-run-go-next] preflight skipped (target ${STAGE} != current ${CURRENT_STAGE})" >&2
    fi
fi

if [[ "$JSON" -eq 1 && "$AUTO_COMMIT" -eq 1 ]]; then
    echo "error: --json is not supported with --commit in this command" >&2
    exit 1
fi

if [[ "$JSON" -eq 0 ]]; then
    echo "[test-run-go-next] running stage ${STAGE} (${SOURCE})"
fi

if [[ "$AUTO_COMMIT" -eq 1 ]]; then
    MILESTONE_ARGS=(--stage "$STAGE" --test-run)
    if [[ "$AUTO_ADVANCE" -eq 1 ]]; then
        MILESTONE_ARGS+=(--advance)
    fi
    if [[ "$AUTO_PUSH" -eq 1 ]]; then
        MILESTONE_ARGS+=(--push)
    fi
    if [[ -n "$COMMIT_MESSAGE" ]]; then
        MILESTONE_ARGS+=(--commit "$COMMIT_MESSAGE")
    else
        MILESTONE_ARGS+=(--commit)
    fi
    bash tools/milestone.sh "${MILESTONE_ARGS[@]}"
    RUN_RC=$?
else
    if [[ "$JSON" -eq 1 ]]; then
        TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh --json
        RUN_RC=$?
    else
        TEST262_STAGE="$STAGE" bash tools/test-run-stage.sh
        RUN_RC=$?
    fi
fi

if [[ "$RUN_RC" -ne 0 ]]; then
    exit "$RUN_RC"
fi

if [[ "$AUTO_COMMIT" -eq 0 && "$AUTO_ADVANCE" -eq 1 ]]; then
    if [[ "$STAGE" == "$CURRENT_STAGE" ]]; then
        bash tools/advance-stage.sh
    elif [[ "$JSON" -eq 0 ]]; then
        echo "[test-run-go-next] advance skipped (target ${STAGE} != current ${CURRENT_STAGE})" >&2
    fi
fi

exit 0
