#!/usr/bin/env bash
# Show recent milestone log entries for quick triage.
# Usage:
#   bash tools/milestone-timeline.sh            # last 20 events
#   bash tools/milestone-timeline.sh 50         # last 50 events
#   MILESTONE_LOG=/path/to/log bash tools/milestone-timeline.sh
#   bash tools/milestone-timeline.sh --log /path/to/log

set -euo pipefail

LOG_FILE="${MILESTONE_LOG:-./.test262_milestones.log}"
limit="${1:-20}"

while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --log)
            if [[ "${2:-}" == "" ]]; then
                echo "error: --log requires a file path" >&2
                exit 1
            fi
            LOG_FILE="$2"
            shift 2
            ;;
        --*)
            echo "error: unknown argument: $1" >&2
            exit 1
            ;;
        *)
            limit="$1"
            shift
            ;;
    esac
done

if [[ ! -f "$LOG_FILE" ]]; then
    echo "No milestone log found at $LOG_FILE" >&2
    exit 1
fi

if ! [[ "$limit" =~ ^[0-9]+$ ]]; then
    echo "error: limit must be a number" >&2
    exit 1
fi

tail -n "$limit" "$LOG_FILE"
