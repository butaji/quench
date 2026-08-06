#!/usr/bin/env bash
# Rerun a failing test for a stage with full diagnostics.
#
# Usage:
#   bash tools/milestone-rerun.sh                # use current stage
#   bash tools/milestone-rerun.sh --stage 34      # use explicit stage
#   bash tools/milestone-rerun.sh --test tests/... # rerun explicit test file
#   bash tools/milestone-rerun.sh --log /tmp/rerun.log # save diagnostic output
#   bash tools/milestone-rerun.sh -- --filter "some filter"
#   bash tools/milestone-rerun.sh --json            # output JSON summary
#   bash tools/milestone-rerun.sh --json --no-log     # output JSON summary only
#   bash tools/milestone-rerun.sh --json --out /tmp/rerun.json
#   env MILESTONE_RERUN_LOG=/tmp/rerun.log bash tools/milestone-rerun.sh --json
#   env MILESTONE_RERUN_JSON_OUT=/tmp/rerun.json bash tools/milestone-rerun.sh --json

set -euo pipefail

STAGE=""
TEST=""
LOG_FILE="${MILESTONE_RERUN_LOG:-./.test262_milestone_rerun.log}"
EXTRA_ARGS=()
OUTPUT_JSON=0
NO_LOG=0
QUIET=0
JSON_OUT=""

log_msg() {
  if [[ "$QUIET" -eq 0 ]]; then
    echo "$1"
  fi
}
RERUN_ARGS_JSON="[]"

emit_json() {
    local status="$1"
    local payload
    payload="$(python3 - "$STAGE" "$TEST" "$LOG_FILE" "$status" "$RERUN_ARGS_JSON" <<'PY'
import json
import sys

stage = sys.argv[1]
test = sys.argv[2]
log_file = sys.argv[3]
status = sys.argv[4]
rerun_args = json.loads(sys.argv[5])
print(json.dumps({
    "stage": stage,
    "test": test,
    "status": status,
    "log": log_file,
    "rerun_args": rerun_args,
}))
PY
)"
    if [[ -n "$JSON_OUT" ]]; then
        echo "$payload" > "$JSON_OUT"
    else
        echo "$payload"
    fi
}

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
    --stage)
      STAGE="$2"
      shift 2
      ;;
    --test)
      TEST="$2"
      shift 2
      ;;
    --log)
      LOG_FILE="$2"
      shift 2
      ;;
    --stage=*)
      STAGE="${1#--stage=}"
      shift
      ;;
    --test=*)
      TEST="${1#--test=}"
      shift
      ;;
    -h|--help)
      sed -n '1,200p' "$0"
      exit 0
      ;;
    --json)
      OUTPUT_JSON=1
      shift
      ;;
    --no-log)
      NO_LOG=1
      shift
      ;;
    --quiet)
      QUIET=1
      shift
      ;;
    --out)
      JSON_OUT="$2"
      shift 2
      ;;
    --)
      shift
      EXTRA_ARGS=("$@")
      break
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ "$STAGE" == "" && "$TEST" == "" ]]; then
  STAGE="$(bash tools/current-stage.sh)" 
fi

if [[ "$TEST" == "" ]]; then
  if ! [[ "$STAGE" =~ ^[0-9]+$ ]]; then
    echo "error: stage must be numeric when inferred automatically" >&2
    exit 1
  fi

  set +e
  TEST_OUTPUT="$(TEST262_STAGE=$STAGE TEST262_DIGEST=1 TEST262_QUICK=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture 2>&1)"
  TEST_RC=$?
  set -e

  if [[ "$TEST_RC" -ne 0 ]]; then
    TEST="$(echo "$TEST_OUTPUT" | python3 - <<'PY'
import re
import sys
text = sys.stdin.read()
patterns = [
    re.compile(r'"sample_paths"\s*:\s*\[\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r'"path"\s*:\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r'"test"\s*:\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r'"file"\s*:\s*"([^"]+\.js)"', re.DOTALL),
    re.compile(r"FAILED [^\n]*tests/.+?\.js"),
]

candidates = []
for pat in patterns:
    if pat.pattern.startswith('FAILED'):
        for m in pat.finditer(text):
            mm = re.search(r'(tests/.+\.js)', m.group(0))
            if mm:
                cand = mm.group(1)
                if cand not in candidates:
                    candidates.append(cand)
        continue

    for m in pat.finditer(text):
        cand = m.group(1)
        if cand not in candidates:
            candidates.append(cand)

if not candidates:
    raise SystemExit(1)
print(candidates[0])
PY
    )"
  else
    TEST=""
  fi
fi

if [[ "$TEST" == "" ]]; then
  echo "no failing test found for stage ${STAGE}" >&2
  exit 1
fi

if [[ "$TEST" != *.js ]]; then
  echo "invalid test path: $TEST" >&2
  exit 1
fi

if [[ "$NO_LOG" -eq 1 ]]; then
    LOG_FILE=""
else
    log_dir="$(dirname "$LOG_FILE")"
    if [[ ! -d "$log_dir" ]]; then
        mkdir -p "$log_dir"
    fi
fi

if [[ -n "$JSON_OUT" ]]; then
    out_dir="$(dirname "$JSON_OUT")"
    if [[ ! -d "$out_dir" ]]; then
        mkdir -p "$out_dir"
    fi
fi

if [[ "$OUTPUT_JSON" -eq 1 && "$JSON_OUT" == "" ]]; then
    JSON_OUT="${MILESTONE_RERUN_JSON_OUT:-.milestone-rerun-${STAGE}.json}"
fi

if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    RERUN_ARGS_JSON="$(python3 - "${EXTRA_ARGS[@]}" <<'PY'
import json
import sys
print(json.dumps(sys.argv[1:]))
PY
)"
fi

RUN_STATUS=0
if [[ "$OUTPUT_JSON" -eq 1 && "$NO_LOG" -eq 1 ]]; then
    cargo run --bin run-test -- --show-script "${EXTRA_ARGS[@]}" "$TEST"
    RUN_STATUS=$?
else
    {
        if [[ "$QUIET" -eq 0 ]]; then
          echo "[milestone-rerun] $(date +'%Y-%m-%d %H:%M:%S%z') Running diagnostics for ${TEST}"
        fi
        cargo run --bin run-test -- --show-script "${EXTRA_ARGS[@]}" "$TEST"
    } | tee -a "$LOG_FILE"
    RUN_STATUS="${PIPESTATUS[0]}"
fi

if [[ "$RUN_STATUS" -ne 0 ]]; then
    if [[ "$OUTPUT_JSON" -eq 1 ]]; then
        emit_json "fail"
    else
        log_msg "[milestone-rerun] Diagnostics failed for ${TEST}."
    fi
    exit 1
fi

if [[ "$OUTPUT_JSON" -eq 1 ]]; then
    emit_json "pass"
else
    if [[ -n "$LOG_FILE" ]]; then
        log_msg "[milestone-rerun] Diagnostics saved to ${LOG_FILE}."
    fi
fi
