#!/usr/bin/env bash
set -euo pipefail

# Fastest path to run the next target stage in one command.
# Usage:
#   bash tools/test-run-go-next-fast.sh
#   bash tools/test-run-go-next-fast.sh --json
#   bash tools/test-run-go-next-fast.sh --no-preflight
#   bash tools/test-run-go-next-fast.sh --by-ratio --top 5
#   bash tools/test-run-go-next-fast.sh --stage 42

if [[ $# -eq 0 ]]; then
    bash tools/test-run-go-next.sh --run --json --advance
    exit 0
fi

bash tools/test-run-go-next.sh --run --json --advance "$@"
