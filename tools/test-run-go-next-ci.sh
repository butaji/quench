#!/usr/bin/env bash
set -euo pipefail

# CI-safe one-shot next-stage readiness gate.
# Usage:
#   bash tools/test-run-go-next-ci.sh
#   bash tools/test-run-go-next-ci.sh --by-ratio --top 5
#   bash tools/test-run-go-next-ci.sh --stage 42

bash tools/test-run-go-next-dryrun.sh --assert-ready --print-json "$@"
