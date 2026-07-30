#!/usr/bin/env bash
# Print the next pending stage id from tasks/index.json.
# Usage:
#   bash tools/next-stage.sh
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    sed -n '1,120p' "$0"
    exit 0
fi

if [[ ${#} -gt 0 ]]; then
    echo "error: next-stage.sh accepts no positional arguments" >&2
    exit 1
fi

NEXT_STAGE="$(bash tools/stage-status.sh --next-id)"
if [[ "$NEXT_STAGE" == "0" ]]; then
    echo "No pending stage." >&2
    exit 1
fi

echo "$NEXT_STAGE"
