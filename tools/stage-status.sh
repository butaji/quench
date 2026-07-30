#!/bin/bash
# Quick overview of all stages status.
# Usage: bash tools/stage-status.sh
#
# Shows pass rate for each stage based on index.json status,
# plus quick-test for the current stage.

cd "$(dirname "$0")/.."

if [[ "${1:-}" == "--json" ]]; then
    if ! bash tools/stage-stats.sh --json; then
        echo "error: failed to read/parse tasks/index.json" >&2
        exit 1
    fi
    exit 0
fi

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    sed -n '1,120p' "$0"
    exit 0
fi

if ! bash tools/stage-stats.sh; then
    echo "error: failed to read/parse tasks/index.json" >&2
    exit 1
fi
