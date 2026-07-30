#!/usr/bin/env bash
set -euo pipefail

# `SSOT == test-run`.
# This script is a small compatibility wrapper around `tools/ssot` and
# `tools/test-run-status-summary.sh`.

NEXT=0
JSON=0
RAW=0
CI=0
SUMMARY=0

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
