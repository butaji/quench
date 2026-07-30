#!/usr/bin/env bash
set -euo pipefail

# Legacy compatibility wrapper.
# Canonical status/run/commit flow is `bash tools/ssot`.
# This script intentionally forwards status to keep older call sites working.

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
      # compatibility-only flag; retained for older call sites
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

if [[ "$RAW" -eq 1 ]]; then
  if [[ "$CI" -eq 1 || "$SUMMARY" -eq 1 || "$JSON" -eq 1 ]]; then
    echo "error: --raw is mutually exclusive with --json/--summary/--ci" >&2
    exit 1
  fi
fi

if [[ "$NEXT" -eq 1 ]]; then
  if [[ "$JSON" -eq 1 ]]; then
    bash tools/test-run-go-next.sh --status --json
  else
    bash tools/test-run-go-next.sh --status
  fi
  exit 0
fi

if [[ "$SUMMARY" -eq 1 && "$JSON" -eq 0 ]]; then
  echo "error: --summary requires --json (consistency with ssot output)" >&2
  exit 1
fi

if [[ "$JSON" -eq 1 ]]; then
  if [[ "$CI" -eq 1 ]]; then
    bash tools/ssot --status --json | python3 - <<'PY'
import json
import sys

payload = json.loads(sys.stdin.read() or "{}").get("test_run_status_summary", {})
blocked = bool(payload.get("is_blocked", False))
has_next = bool(payload.get("has_next", False))
if blocked or has_next:
    print(json.dumps(payload, sort_keys=True))
    raise SystemExit(1)
print(json.dumps(payload, sort_keys=True))
PY
  else
    bash tools/ssot --status --json
  fi
  exit 0
fi

if [[ "$RAW" -eq 1 ]]; then
  bash tools/ssot --status --json | python3 - <<'PY'
import json
import sys

payload = json.loads(sys.stdin.read() or "{}").get("test_run_status_summary", {})
print(f"Current stage: {payload.get('current_stage')}")
print(f"Path: {payload.get('path', '')}")
print(f"Status: {payload.get('status')}")
print(f"Tests: {payload.get('tests')}")
print(f"Failed: {payload.get('failed')}")
print(f"Has next: {1 if payload.get('has_next') else 0}")
next_stage = payload.get('next_stage', {}) or {}
if next_stage:
    print(f"Next stage: {next_stage.get('id')} ({next_stage.get('status', 'pending')})")
PY
  exit 0
fi

if [[ "$CI" -eq 1 ]]; then
  bash tools/ssot --status --json | python3 - <<'PY'
import json
import sys
payload = json.loads(sys.stdin.read() or "{}").get("test_run_status_summary", {})
print(payload.get("is_blocked", True) and "not ready" or "ready")
sys.exit(1 if payload.get("is_blocked") else 0)
PY
  exit 0
fi

if [[ "$SUMMARY" -eq 1 ]]; then
  echo "error: --summary requires --json" >&2
  exit 1
fi

bash tools/ssot --status
