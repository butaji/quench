#!/usr/bin/env bash
set -euo pipefail

# Canonical wrapper for plan status in the active SSOT flow.
# Usage:
#   bash tools/run-test-plan-status.sh
#   bash tools/run-test-plan-status.sh --batch
#   bash tools/run-test-plan-status.sh --batch --ratio --top 5
#   bash tools/run-test-plan-status.sh --batch --ratio --top 5 --raw

ARGS=()
RAW=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --raw)
            RAW=1
            shift
            ;;
        -h|--help)
            sed -n '1,120p' "$0"
            exit 0
            ;;
        --status-json)
            # Idempotent: already on the canonical normalized path.
            shift
            ;;
        *)
            ARGS+=("$1")
            shift
            ;;
    esac
done

if [[ "$RAW" -eq 1 ]]; then
    bash tools/run-test-plan.sh --status --json --raw "${ARGS[@]}"
else
    bash tools/run-test-plan.sh --status-json "${ARGS[@]}"
fi

