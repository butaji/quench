#!/usr/bin/env bash
set -euo pipefail

# `SSOT == test-run`.
# Canonical workflow helper for implementation checkpoints.

NEXT=0
JSON=0
RAW=0
CI=0
SUMMARY=0
RUN=0
COMMIT=0
PUSH=0
FAST=1
COMMIT_MESSAGE=""

while [[ ${#} -gt 0 ]]; do
  case "${1:-}" in
    --next)
      NEXT=1
      shift
      ;;
    --status)
      # compatibility-only; status mode is ssot default
      shift
      ;;
    --run)
      RUN=1
      shift
      ;;
    --fast)
      FAST=1
      shift
      ;;
    --slow)
      FAST=0
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
    --json)
      JSON=1
      shift
      ;;
    --raw)
      RAW=1
      shift
      ;;
    --ci)
      CI=1
      shift
      ;;
    --summary)
      SUMMARY=1
      shift
      ;;
    -h|--help)
      sed -n '1,140p' "$0"
      exit 0
      ;;
    *)
      echo "error: unexpected argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ "$RUN" -eq 1 && "$COMMIT" -eq 1 && "$PUSH" -eq 0 ]]; then
  if [[ "$COMMIT_MESSAGE" != "" ]]; then
    echo "error: --commit requires --push in this script for explicit milestone flow" >&2
    echo "use: --run --commit --push, or remove --commit" >&2
  else
    echo "error: --commit requires --push in this script for explicit milestone flow" >&2
    echo "use: --run --commit --push, or remove --commit" >&2
  fi
  exit 1
fi

if [[ "$PUSH" -eq 1 && "$COMMIT" -eq 0 ]]; then
  echo "error: --push requires --commit" >&2
  exit 1
fi

if [[ "$RAW" -eq 1 && ("$JSON" -eq 1 || "$SUMMARY" -eq 1 || "$CI" -eq 1) ]]; then
  echo "error: --raw is mutually exclusive with --json/--summary/--ci" >&2
  exit 1
fi

if [[ "$SUMMARY" -eq 1 && "$JSON" -eq 0 ]]; then
  echo "error: --summary requires --json" >&2
  exit 1
fi

if [[ "$NEXT" -eq 1 ]]; then
  if [[ "$JSON" -eq 1 ]]; then
    bash tools/ssot --next --status --json
  else
    bash tools/ssot --next --status
  fi
  exit 0
fi

if [[ "$RUN" -eq 1 ]]; then
  if [[ "$COMMIT" -eq 1 ]]; then
    SSOT_RUN_ARGS=(--run --commit)
    if [[ -n "$COMMIT_MESSAGE" ]]; then
      SSOT_RUN_ARGS+=(--commit "$COMMIT_MESSAGE")
    fi
    if [[ "$PUSH" -eq 1 ]]; then
      SSOT_RUN_ARGS+=(--push)
    fi
    if [[ "$FAST" -eq 1 ]]; then
      SSOT_RUN_ARGS+=(--fast)
    fi
    bash tools/ssot "${SSOT_RUN_ARGS[@]}"
    exit $?
  fi

  if [[ "$FAST" -eq 1 ]]; then
    bash tools/ssot --run --fast
  else
    bash tools/ssot --run
  fi
  exit 0
fi

if [[ "$SUMMARY" -eq 1 ]]; then
  bash tools/test-run-status-summary.sh --json
  exit 0
fi

if [[ "$RAW" -eq 1 ]]; then
  bash tools/test-run-status-summary.sh
  exit 0
fi

if [[ "$CI" -eq 1 ]]; then
  if bash tools/test-run-status-summary.sh --blocker >/dev/null; then
    echo "ready"
    exit 0
  fi
  echo "not ready"
  exit 1
fi

if [[ "$JSON" -eq 1 ]]; then
  bash tools/ssot --status --json
  exit 0
fi

bash tools/ssot --status
