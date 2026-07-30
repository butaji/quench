#!/usr/bin/env bash
# SSOT = test-run stage execution.
# Compatibility wrapper for historical `ssot` command names.
# This file is a strict alias for `test-run-stage.sh`.
set -euo pipefail

case "${SSOT_BUILD_RUN_TEST:-}" in
  [Tt]rue|[Tt][Rr][Uu][Ee]|1|[Yy][Ee][Ss]|yes|[Yy])
  export TEST262_TEST_RUN_BUILD=1
  ;;
esac

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/test-run-stage.sh" "$@"
