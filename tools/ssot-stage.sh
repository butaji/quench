#!/usr/bin/env bash
# Compatibility wrapper retained for legacy callers.
# Deprecated alias for the canonical command: test-run-stage.sh.
# This wrapper exists only for legacy callers.
set -euo pipefail

echo "[ssot-stage] Deprecated: use test-run-stage.sh instead." >&2

if [[ "${SSOT_BUILD_RUN_TEST:-0}" == "1" ]]; then
  # Legacy environment alias for prebuild mode.
  export TEST262_TEST_RUN_BUILD=1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$SCRIPT_DIR/test-run-stage.sh" "$@"
