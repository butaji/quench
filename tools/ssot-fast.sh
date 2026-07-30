#!/usr/bin/env bash
set -euo pipefail

# SSOT fast workflow: single entrypoint for test-run status/run/commit/push.
# Usage:
#   bash tools/ssot-fast.sh                       # show canonical status
#   bash tools/ssot-fast.sh --run                 # run current stage quickly
#   bash tools/ssot-fast.sh --run --commit         # run and commit if clean
#   bash tools/ssot-fast.sh --run --commit --push  # run/commit/push current milestone
#   bash tools/ssot-fast.sh --next --run           # run next pending stage quickly
#   bash tools/ssot-fast.sh --next --run --commit --push
#   bash tools/ssot-fast.sh --next --run --message "..." # custom commit message

NEXT=0
RUN=0
COMMIT=0
PUSH=0
COMMIT_MESSAGE=""

while [[ ${#} -gt 0 ]]; do
  case "${1:-}" in
    --next)
      NEXT=1
      shift
      ;;
    --run)
      RUN=1
      shift
      ;;
    --commit)
      COMMIT=1
      if [[ ${2:-} != --* && ${2:-} != "" ]]; then
        COMMIT_MESSAGE="$2"
        shift 2
      else
        shift
      fi
      ;;
    --push)
      PUSH=1
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

if [[ "$PUSH" -eq 1 && "$COMMIT" -eq 0 ]]; then
  echo "error: --push requires --commit" >&2
  exit 1
fi

if [[ "$COMMIT" -eq 1 && "$RUN" -eq 0 ]]; then
  echo "error: --commit requires --run" >&2
  exit 1
fi

if [[ "$RUN" -eq 0 ]]; then
  bash tools/ssot
  exit 0
fi

if [[ "$NEXT" -eq 1 ]]; then
  if [[ "$COMMIT" -eq 1 ]]; then
    PUSH_ARGS=()
    if [[ "$PUSH" -eq 1 ]]; then
      PUSH_ARGS=(--push)
    fi
    if [[ -n "$COMMIT_MESSAGE" ]]; then
      bash tools/ssot --next --run --fast --commit "$COMMIT_MESSAGE" "${PUSH_ARGS[@]}"
    else
      bash tools/ssot --next --run --fast --commit "${PUSH_ARGS[@]}"
    fi
  else
    bash tools/ssot --next --run --fast
  fi
  exit 0
fi

if [[ "$COMMIT" -eq 1 ]]; then
  PUSH_ARGS=()
  if [[ "$PUSH" -eq 1 ]]; then
    PUSH_ARGS=(--push)
  fi
  if [[ -n "$COMMIT_MESSAGE" ]]; then
    bash tools/ssot --run --fast --commit "$COMMIT_MESSAGE" "${PUSH_ARGS[@]}"
  else
    bash tools/ssot --run --fast --commit "${PUSH_ARGS[@]}"
  fi
else
  bash tools/ssot --run --fast
fi
