#!/usr/bin/env bash
set -euo pipefail

stage="${1:-${TEST262_STAGE:-}}"
if [[ -z "$stage" ]]; then
  echo "Usage: $0 <stage>" >&2
  echo "Or: TEST262_STAGE=<stage> $0" >&2
  exit 1
fi

shift || true

if [[ "${SSOT_BUILD_RUN_TEST:-0}" == "1" ]]; then
  cargo build --bin run-test --quiet
fi

echo "[ssot-stage] Stage $stage (digest)"
TEST262_STAGE="$stage" TEST262_DIGEST=1 TEST262_QUICK=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture "$@"
