#!/usr/bin/env bash
set -euo pipefail

# Single command status snapshot for implementation progress.
# Usage:
#   bash tools/implementation-progress.sh
#   bash tools/implementation-progress.sh --next
#   bash tools/implementation-progress.sh --raw

RAW=0
INCLUDE_NEXT=0

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --raw)
            RAW=1
            shift
            ;;
        --next)
            INCLUDE_NEXT=1
            shift
            ;;
        -h|--help)
            sed -n '1,120p' "$0"
            exit 0
            ;;
        *)
            echo "error: unexpected argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ "$RAW" -eq 1 ]]; then
    bash tools/stage-status.sh --current
    if [[ "$INCLUDE_NEXT" -eq 1 ]]; then
        echo
        bash tools/stage-status.sh --next
    fi
    exit 0
fi

if ! CURRENT_JSON=$(bash tools/stage-status.sh --json --current); then
    echo "error: failed to read current stage status" >&2
    exit 1
fi
NEXT_JSON=''
if [[ "$INCLUDE_NEXT" -eq 1 ]]; then
    if ! NEXT_JSON=$(bash tools/stage-status.sh --json --next); then
        echo "error: failed to read next stage status" >&2
        exit 1
    fi
fi

python3 - "$CURRENT_JSON" "$NEXT_JSON" "$INCLUDE_NEXT" <<'PY'
import json
import sys

current_json = sys.argv[1]
next_json = sys.argv[2]
include_next = sys.argv[3] == "1"

current_payload = json.loads(current_json)
next_payload = json.loads(next_json) if next_json else None

payload = {
    "implementation": {
        "current": current_payload,
    }
}

if include_next:
    payload["implementation"]["next"] = next_payload

print(json.dumps(payload, sort_keys=True))
PY
