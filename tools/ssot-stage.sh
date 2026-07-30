#!/usr/bin/env bash
# Compatibility wrapper retained for legacy callers.
# Canonical term and command are "test-run".
# This wrapper is deprecated and exists only for legacy callers.
set -euo pipefail

if [[ "${SSOT_BUILD_RUN_TEST:-0}" == "1" ]]; then
  # Legacy environment alias for prebuild mode.
  export TEST262_TEST_RUN_BUILD=1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$SCRIPT_DIR/test-run-stage.sh" "$@"
