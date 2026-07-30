#!/usr/bin/env bash
# Compatibility wrapper retained for legacy callers.
# Canonical command is `tools/test-run-stage.sh`.
# Terminology is now "test-run".
# This wrapper is deprecated and exists only for legacy callers.
set -euo pipefail

if [[ "${SSOT_BUILD_RUN_TEST:-0}" == "1" ]]; then
  echo "[ssot-stage] warning: SSOT_BUILD_RUN_TEST is deprecated; use TEST262_TEST_RUN_BUILD=1" >&2
  export TEST262_TEST_RUN_BUILD=1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$SCRIPT_DIR/test-run-stage.sh" "$@"
