#!/usr/bin/env bash
# Compatibility wrapper retained for legacy `ssot` callers.
# Canonical command: test-run-stage.sh
# This wrapper maps deprecated `ssot` aliases to `test-run`.
set -euo pipefail

echo "[legacy-ssot] deprecated: use test-run-stage.sh instead." >&2

case "${SSOT_BUILD_RUN_TEST:-}" in
  [Tt]rue|[Tt][Rr][Uu][Ee]|1|yes|[Yy][Ee][Ss])
  # Legacy environment alias for prebuild mode.
  echo "[legacy-ssot] Deprecated env var: SSOT_BUILD_RUN_TEST=1 is deprecated; use TEST262_TEST_RUN_BUILD=1." >&2
  export TEST262_TEST_RUN_BUILD=1
  ;;
esac

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$SCRIPT_DIR/test-run-stage.sh" "$@"
