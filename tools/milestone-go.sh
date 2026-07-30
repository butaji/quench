#!/usr/bin/env bash
set -euo pipefail

# Canonical test-run checkpoint command.
# Usage:
#   bash tools/milestone-go.sh
#   bash tools/milestone-go.sh --message "feat: ..."
#   bash tools/milestone-go.sh --push
#   bash tools/milestone-go.sh --json
#
# Runs the next target stage via test-run, requires it to pass, advances
# current_stage on success, and optionally commits/pushes milestone updates.
# `ssot` is a legacy alias name for this same flow.

JSON=0
PUSH=0
MESSAGE=""

while [[ ${#} -gt 0 ]]; do
  case "${1:-}" in
    --json)
      JSON=1
      shift
      ;;
    --push)
      PUSH=1
      shift
      ;;
    --message)
      if [[ ${2:-} == "" || ${2:-} == --* ]]; then
        echo "error: --message requires a text argument" >&2
        exit 1
      fi
      MESSAGE="$2"
      shift 2
      ;;
    -h|--help)
      sed -n '1,200p' "$0"
      exit 0
      ;;
    *)
      echo "error: unexpected argument: $1" >&2
      exit 1
      ;;
  esac
done

ARGS=(--run --advance)
if [[ "$JSON" -eq 1 ]]; then
  ARGS+=(--json)
fi
if [[ -n "$MESSAGE" ]]; then
  ARGS+=(--commit "$MESSAGE")
else
  ARGS+=(--commit)
fi
if [[ "$PUSH" -eq 1 ]]; then
  ARGS+=(--push)
fi

bash tools/test-run-go-next.sh "${ARGS[@]}"
