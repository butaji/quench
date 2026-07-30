#!/usr/bin/env bash
# SSOT == test run stage execution.
# Canonical (fastest understood) entrypoint for running the active stage.
# `ssot-stage.sh` and `test-run-stage.sh` are strict aliases.
set -euo pipefail

case "${SSOT_BUILD_RUN_TEST:-}" in
  [Tt]rue|[Tt][Rr][Uu][Ee]|1|[Yy][Ee][Ss]|yes|[Yy])
  export TEST262_TEST_RUN_BUILD=1
  ;;
esac
case "${TEST262_TEST_RUN_BUILD:-0}" in
  [Tt]rue|[Tt][Rr][Uu][Ee]|1|[Yy][Ee][Ss]|yes|[Yy])
  export TEST262_TEST_RUN_BUILD=1
  ;;
esac

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/test-run-stage.sh" "$@"
