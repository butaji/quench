#!/usr/bin/env bash
set -euo pipefail
# Canonical runner for a stage test-run (legacy `ssot-stage.sh` delegates here).
# Usage:
#   bash tools/test-run-stage.sh                  # run current stage test-run
#   bash tools/test-run-stage.sh --stage 42        # explicit stage
#   TEST262_TEST_RUN_BUILD=1 bash tools/test-run-stage.sh  # prebuild run-test before running

STAGE="${TEST262_STAGE:-$(python3 -c "import json; print(json.load(open('tasks/index.json'))['current_stage'])")}"
AUTO_HELP=0

if [[ ${#} -gt 0 && ("${1:-}" == --help || "${1:-}" == -h) ]]; then
    AUTO_HELP=1
fi
if [[ "$AUTO_HELP" -eq 1 ]]; then
    sed -n '1,120p' "$0"
    exit 0
fi

if [[ "${1:-}" == --stage ]]; then
    STAGE="$2"
    shift 2
else
    STAGE="${1:-$STAGE}"
    shift || true
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

echo "[test-run-stage] Stage $STAGE (digest)"
TEST262_STAGE="$STAGE" TEST262_DIGEST=1 TEST262_QUICK=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture "$@"

