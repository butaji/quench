#!/usr/bin/env bash
# Compatibility wrapper for historical `ssot` command names.
# Canonical command remains `test-run-stage.sh` (test-run).
# `ssot` is treated as a strict alias of test-run-stage.
set -euo pipefail

case "${SSOT_BUILD_RUN_TEST:-}" in
  [Tt]rue|[Tt][Rr][Uu][Ee]|1|[Yy][Ee][Ss]|yes|[Yy])
  export TEST262_TEST_RUN_BUILD=1
  ;;
esac

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/test-run-stage.sh" "$@"
