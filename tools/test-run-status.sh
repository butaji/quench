#!/usr/bin/env bash
set -euo pipefail

# Canonical wrapper for stage test-run status output.
# Usage:
#   bash tools/test-run-status.sh
#   bash tools/test-run-status.sh --stage 42
#   bash tools/test-run-status.sh --raw

RAW=0
ARGS=()

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --raw)
            RAW=1
            shift
            ;;
        --json)
            # Idempotent: machine-readable mode is already explicit.
            shift
            ;;
        -h|--help)
            sed -n '1,120p' "$0"
            exit 0
            ;;
        *)
            ARGS+=("$1")
            shift
            ;;
    esac
done

if [[ "$RAW" -eq 1 ]]; then
    bash tools/test-run-stage.sh "${ARGS[@]}"
else
    bash tools/test-run-stage.sh --json "${ARGS[@]}"
fi
