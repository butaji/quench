#!/usr/bin/env bash
set -euo pipefail

# CI-safe one-shot next-stage readiness gate.
# Usage:
#   bash tools/test-run-go-next-ci.sh [--json | --json-only] [--by-ratio --top 5]
#   bash tools/test-run-go-next-ci.sh --json-only --stage 42

JSON=0
ARGS=("--assert-ready")

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json|--json-only)
            JSON=1
            shift
            ;;
        -h|--help)
            sed -n '1,220p' "$0"
            exit 0
            ;;
        *)
            ARGS+=("$1")
            shift
            ;;
    esac
done

if [[ "$JSON" -eq 1 ]]; then
    bash tools/test-run-go-next-dryrun.sh --print-json "${ARGS[@]}"
else
    bash tools/test-run-go-next-dryrun.sh "${ARGS[@]}"
fi
