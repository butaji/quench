#!/usr/bin/env bash
# Canonical wrapper for running a stage test-run (legacy alias: ssot-stage.sh)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$SCRIPT_DIR/ssot-stage.sh" "$@"
