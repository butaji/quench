#!/usr/bin/env bash
set -euo pipefail

# SSOT-fast milestone helper.
# Canonical source of truth is SSOT; this wrapper keeps the common milestone loop
# short when you are moving stage-by-stage.
#
# Usage:
#   bash tools/milestone-run.sh                         # show SSOT status
#   bash tools/milestone-run.sh --run                   # run current stage fast
#   bash tools/milestone-run.sh --run --commit [--push]  # run, commit, optional push
#   bash tools/milestone-run.sh --next --run             # run next pending stage fast
#   bash tools/milestone-run.sh --next --run --commit     # run next and commit
#   bash tools/milestone-run.sh --next --run --commit --push

NEXT=0
RUN=0
COMMIT=0
PUSH=0

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
      shift
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
  bash tools/ssot --status
  exit 0
fi

SSOT_ARGS=(--fast)
if [[ "$COMMIT" -eq 1 ]]; then
  SSOT_ARGS+=(--commit)
fi
if [[ "$PUSH" -eq 1 ]]; then
  SSOT_ARGS+=(--push)
fi

if [[ "$NEXT" -eq 1 ]]; then
  bash tools/ssot --next --run "${SSOT_ARGS[@]}"
else
  bash tools/ssot --run "${SSOT_ARGS[@]}"
fi
