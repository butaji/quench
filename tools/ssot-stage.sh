#!/usr/bin/env bash
set -euo pipefail

stage="${1:-${TEST262_STAGE:-}}"
if [[ -z "$stage" ]]; then
  echo "Usage: $0 <stage>" >&2
  echo "Or: TEST262_STAGE=<stage> $0" >&2
  exit 1
fi
shift

TEST262_STAGE="$stage" TEST262_DIGEST=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture "$@"
