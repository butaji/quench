#!/usr/bin/env bash
set -euo pipefail

STAGE="$(bash tools/current-stage.sh)"
MESSAGE=""
PUSH=0

while [[ ${#} -gt 0 ]]; do
  case "${1:-}" in
    --push)
      PUSH=1
      shift
      ;;
    --message)
      if [[ "${2:-}" == "" ]]; then
        echo "error: --message requires an argument" >&2
        exit 1
      fi
      MESSAGE="$2"
      shift 2
      ;;
    --help|-h)
      echo "Usage: tools/test-run-milestone.sh [--message \"text\"] [--push]"
      echo "Run current stage test-run, commit, and optionally push."
      exit 0
      ;;
    *)
      echo "error: unexpected argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ "$MESSAGE" == "" ]]; then
  MESSAGE="ssot milestone stage ${STAGE}"
fi

ARGS=(--stage "$STAGE" --test-run --commit "$MESSAGE")
if [[ "$PUSH" -eq 1 ]]; then
  ARGS+=(--push)
fi

bash tools/milestone.sh "${ARGS[@]}"
