#!/usr/bin/env bash
set -euo pipefail
# Canonical runner for a stage test-run (legacy `ssot-stage.sh` delegates here).
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
  EXTRA_ENV_ARGS=(TEST262_DIGEST=1 TEST262_QUICK=1 TEST262_JSON=1)
else
  EXTRA_ENV_ARGS=(TEST262_DIGEST=1 TEST262_QUICK=1)
fi

if [[ "${TEST262_TEST_RUN_BUILD:-0}" == "1" ]]; then
  cargo build --bin run-test --quiet
fi

if [[ "$OUTPUT_JSON" -eq 0 ]]; then
  echo "[test-run-stage] Stage $STAGE (digest)"
fi

TEST262_STAGE="$STAGE" "${EXTRA_ENV_ARGS[@]}" cargo test -p quench-runtime --test test262 test262_staged -- --nocapture "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"
