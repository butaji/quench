#!/usr/bin/env bash
set -euo pipefail
# Canonical runner for a stage test-run.
# Usage:
#   bash tools/test-run-stage.sh                  # run current stage test-run
#   bash tools/test-run-stage.sh --stage 42        # explicit stage
#   TEST262_TEST_RUN_BUILD=1 bash tools/test-run-stage.sh  # prebuild run-test before running
#   bash tools/test-run-stage.sh --json            # emit machine-readable stage result

STAGE="$(bash tools/current-stage.sh)"
OUTPUT_JSON=0
EXTRA_ARGS=()

if [[ ${#} -gt 0 && ("${1:-}" == --help || "${1:-}" == -h) ]]; then
    sed -n '1,120p' "$0"
    exit 0
fi

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json)
            OUTPUT_JSON=1
            shift
            ;;
        --stage)
            STAGE="$2"
            shift 2
            ;;
        --)
            shift
            EXTRA_ARGS=("$@")
            set --
            ;;
        -h|--help)
            sed -n '1,120p' "$0"
            exit 0
            ;;
        *)
            if [[ "${1:-}" =~ ^[0-9]+$ ]]; then
                STAGE="${1:-$STAGE}"
                shift
                EXTRA_ARGS=("$@")
                set --
            else
                echo "error: unexpected argument: $1" >&2
                exit 1
            fi
            ;;
    esac
done

if [[ "$#" -gt 0 ]]; then
    echo "error: unexpected trailing arguments: $*" >&2
    exit 1
fi

if [[ -z "$STAGE" ]]; then
  echo "Usage: $0 [--stage N] " >&2
  echo "Canonical term: test-run" >&2
  echo "Or: TEST262_STAGE=<stage> $0" >&2
  echo "Or: $0 <stage>" >&2
    exit 1
fi

if [[ "$OUTPUT_JSON" -eq 1 ]]; then
  EXTRA_ENV_ARGS=(TEST262_DIGEST=1 TEST262_QUICK=1)
else
  EXTRA_ENV_ARGS=(TEST262_DIGEST=1 TEST262_QUICK=1)
fi

if [[ "${TEST262_TEST_RUN_BUILD:-0}" == "1" ]]; then
  cargo build --bin run-test --quiet
fi

if [[ "$OUTPUT_JSON" -eq 0 ]]; then
  echo "[test-run-stage] Stage $STAGE (digest)"
  env TEST262_STAGE="$STAGE" "${EXTRA_ENV_ARGS[@]}" cargo test -p quench-runtime --test test262 test262_staged -- --nocapture "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"
  exit $?
fi

TMP_OUTPUT="$(mktemp)"
set +e
env TEST262_STAGE="$STAGE" "${EXTRA_ENV_ARGS[@]}" cargo test -p quench-runtime --test test262 test262_staged -- --nocapture "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}" > "$TMP_OUTPUT" 2>&1
RUN_RC=$?
set -e

python3 - "$TMP_OUTPUT" "$RUN_RC" "$STAGE" <<'PY'
import json
import re
import sys

output_path, run_rc, stage = sys.argv[1], int(sys.argv[2]), sys.argv[3]
text = open(output_path, "r", encoding="utf-8", errors="replace").read()

def as_int(value):
    return int(value) if value is not None else 0

payload = {
    "stage": int(stage),
    "run_rc": run_rc,
    "passed": 0,
    "failed": 0,
    "skipped": 0,
    "total": 0,
    "first_failure": None,
    "has_blocker": False,
}

complete = re.search(
    r"ALL STAGES COMPLETE — Stage (?P<stage>\d+): (?P<passed>\d+)/(?P<total>\d+) \(skipped (?P<skipped>\d+)\)",
    text,
)
if complete is None:
    complete = re.search(
        r"Stage (?P<stage>\d+): (?P<passed>\d+)/(?P<total>\d+) passed, (?P<skipped>\d+) skipped",
        text,
    )

if complete:
    payload["passed"] = as_int(complete.groupdict().get("passed"))
    payload["total"] = as_int(complete.groupdict().get("total"))
    payload["skipped"] = as_int(complete.groupdict().get("skipped"))
    if run_rc != 0:
        payload["failed"] = 1
        payload["has_blocker"] = True
else:
    payload["failed"] = 1
    payload["has_blocker"] = True

if run_rc != 0:
    payload["failed"] = 1
    payload["has_blocker"] = True

if payload["total"] == 0:
    count_match = re.search(r"\((\d+) tests\)", text)
    if count_match:
        payload["total"] = as_int(count_match.group(1))

first_failure = re.search(
    r"FIRST FAILURE\n\s*Stage (?P<stage>\d+) \| #(?P<index>\d+)\n\s*(?P<path>[^\n]+\.js)\n\s*Type:\s*(?P<type>[^\n]+)\n\s*Reason:\s*(?P<reason>[^\n]+)",
    text,
)
if first_failure:
    payload["first_failure"] = {
        "stage": int(first_failure.group("stage")),
        "index": int(first_failure.group("index")),
        "path": first_failure.group("path").strip(),
        "type": first_failure.group("type").strip(),
        "reason": first_failure.group("reason").strip(),
    }

print(json.dumps({"test_run_stage": payload}, sort_keys=True))
PY
RUN_JSON_RC=$?
rm -f "$TMP_OUTPUT"
if [[ "$RUN_JSON_RC" -ne 0 ]]; then
  exit "$RUN_JSON_RC"
fi
if [[ "$RUN_RC" -ne 0 ]]; then
  exit "$RUN_RC"
fi
exit 0
