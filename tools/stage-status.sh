#!/bin/bash
# Quick overview of all stages status.
# Usage: bash tools/stage-status.sh
# Usage: bash tools/stage-status.sh --current
# Usage: bash tools/stage-status.sh --json --current
# Usage: bash tools/stage-status.sh --current --json
# Usage: bash tools/stage-status.sh --next
# Usage: bash tools/stage-status.sh --json --next
# Usage: bash tools/stage-status.sh --next-id
#
# Shows stage status from tasks/index.json (derived from test-run updates).

cd "$(dirname "$0")/.."

if [[ ${#} -gt 0 ]]; then
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
      sed -n '1,120p' "$0"
      exit 0
  fi

  STAGE_STATUS_ARGS=()
  while [[ ${#} -gt 0 ]]; do
    case "${1:-}" in
        --json|--current|--next|--next-id)
            STAGE_STATUS_ARGS+=("${1}")
            shift
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            exit 1
            ;;
    esac
  done

  if ! bash tools/stage-stats.sh "${STAGE_STATUS_ARGS[@]}"; then
      echo "error: failed to read/parse tasks/index.json" >&2
      exit 1
  fi
  exit 0
fi

if ! bash tools/stage-stats.sh; then
    echo "error: failed to read/parse tasks/index.json" >&2
    exit 1
fi
