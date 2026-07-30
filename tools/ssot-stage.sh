#!/usr/bin/env bash
# Compatibility wrapper retained for legacy callers: use `test-run-stage.sh`.
set -euo pipefail

if [[ "${SSOT_BUILD_RUN_TEST:-0}" == "1" ]]; then
  export TEST262_TEST_RUN_BUILD=1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$SCRIPT_DIR/test-run-stage.sh" "$@"

