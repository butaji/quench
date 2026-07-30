#!/bin/bash
# Quick overview of all stages status.
# Usage: bash tools/stage-status.sh
#
# Shows pass rate for each stage based on index.json status,
# plus quick-test for the current stage.

cd "$(dirname "$0")/.."

if ! bash tools/stage-stats.sh; then
    echo "error: failed to read/parse tasks/index.json" >&2
    exit 1
fi
