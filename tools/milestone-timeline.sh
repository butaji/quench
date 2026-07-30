#!/usr/bin/env bash
# Show recent milestone log entries for quick triage.
# Usage:
#   bash tools/milestone-timeline.sh            # last 20 events
#   bash tools/milestone-timeline.sh 50         # last 50 events
#   MILESTONE_LOG=/path/to/log bash tools/milestone-timeline.sh

set -euo pipefail

log_file="${MILESTONE_LOG:-./.test262_milestones.log}"
limit="${1:-20}"

if [[ ! -f "$log_file" ]]; then
    echo "No milestone log found at $log_file" >&2
    exit 1
fi

if ! [[ "$limit" =~ ^[0-9]+$ ]]; then
    echo "error: limit must be a number" >&2
    exit 1
fi

 tail -n "$limit" "$log_file"
