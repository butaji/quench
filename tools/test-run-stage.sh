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
LOG_FILE=""
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
        --log)
            LOG_FILE="$2"
            shift 2
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

if [[ "${TEST262_TEST_RUN_BUILD:-0}" == "1" ]]; then
  cargo build --bin run-test --quiet
fi

if [[ "${SSOT_BUILD_RUN_TEST:-0}" == "1" ]]; then
  export TEST262_TEST_RUN_BUILD=1
fi

# Canonical SSOT runs every test. Set TEST262_QUICK=1 explicitly only for
# diagnostic triage; quick output is never a conformance/progress result.
case "${TEST262_QUICK:-0}" in
  0|false|False|FALSE|no|No|NO|off|Off|OFF)
    EXTRA_ENV_ARGS=(TEST262_DIGEST=1)
    ;;
  *)
    EXTRA_ENV_ARGS=(TEST262_DIGEST=1 TEST262_QUICK=1)
    ;;
esac

NEEDS_JSON_PARSE=0
if [[ "$OUTPUT_JSON" -eq 1 || -n "$LOG_FILE" ]]; then
  NEEDS_JSON_PARSE=1
fi

if [[ "$NEEDS_JSON_PARSE" -eq 0 ]]; then
  echo "[test-run-stage] Stage $STAGE (digest)"
  env TEST262_STAGE="$STAGE" "${EXTRA_ENV_ARGS[@]}" cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"
  exit $?
fi

TMP_OUTPUT="$(mktemp)"
set +e
if [[ "$OUTPUT_JSON" -eq 0 ]]; then
  echo "[test-run-stage] Stage $STAGE (digest)"
  env TEST262_STAGE="$STAGE" "${EXTRA_ENV_ARGS[@]}" cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}" | tee "$TMP_OUTPUT"
else
  env TEST262_STAGE="$STAGE" "${EXTRA_ENV_ARGS[@]}" cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}" > "$TMP_OUTPUT" 2>&1
fi
RUN_RC=$?
set -e

PY_PAYLOAD="$(python3 - "$RUN_RC" "$STAGE" "$TMP_OUTPUT" <<'PY'
import json
import re
import sys

run_rc = int(sys.argv[1])
stage = int(sys.argv[2])
with open(sys.argv[3], encoding="utf-8") as report:
    text = report.read()

def as_int(value):
    return int(value) if value is not None else 0

payload = {
    "stage": stage,
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
    payload["passed"] = as_int(complete.group("passed"))
    payload["total"] = as_int(complete.group(2))
    payload["skipped"] = as_int(complete.group("skipped"))
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
)" || PY_PAYLOAD=""

if [[ -n "$LOG_FILE" ]]; then
  COMMIT_HASH="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  GIT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo detached)"
  STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  PY_OUT="$TMP_OUTPUT.log"
  cat > "$PY_OUT" <<'JSON'
import json,sys
log_file = sys.argv[1]
commit = sys.argv[2]
branch = sys.argv[3]
stamp = sys.argv[4]
run_payload = sys.argv[5]

try:
    with open(log_file, "r", encoding="utf-8") as handle:
        data = json.load(handle)
except Exception:
    data = {"entries": []}

entries = data.get("entries", [])
if not isinstance(entries, list):
    entries = []

try:
    decoded = json.loads(run_payload)
    decoded.setdefault("commit", commit)
    decoded.setdefault("branch", branch)
    decoded.setdefault("stamp", stamp)
    decoded["test_run_stage"]["commit"] = commit
    decoded["test_run_stage"]["branch"] = branch
    decoded["test_run_stage"]["stamp"] = stamp
    entries.append(decoded["test_run_stage"])
except Exception:
    entries.append({
        "commit": commit,
        "branch": branch,
        "stamp": stamp,
        "raw_parse_error": True,
        "raw_output": run_payload[:2000],
    })

data["entries"] = entries
with open(log_file, "w", encoding="utf-8") as handle:
    json.dump(data, handle, sort_keys=True)
    handle.write("\\n")
JSON
  python3 "$PY_OUT" "$LOG_FILE" "$COMMIT_HASH" "$GIT_BRANCH" "$STAMP" "$PY_PAYLOAD"
  rm -f "$PY_OUT"
fi

if [[ "$OUTPUT_JSON" -eq 1 ]]; then
  if [[ -n "$PY_PAYLOAD" ]]; then
    printf '%s\n' "$PY_PAYLOAD"
  else
    echo '{"test_run_stage":{"stage":'"$STAGE"',"run_rc":'"$RUN_RC"',"failed":1,"has_blocker":true}}'
  fi
fi

rm -f "$TMP_OUTPUT"
if [[ "$RUN_RC" -ne 0 ]]; then
  exit "$RUN_RC"
fi
exit 0
