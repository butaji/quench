#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
output=$(CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/audit-value-asm}" \
  "$root/tools/audit/audit-value-assembly.sh")
case "$output" in
  *"inline/cold contracts ok (11 inline, 5 cold)"*) ;;
  *) printf 'compiler output contract failed: %s\n' "$output" >&2; exit 1 ;;
esac
case "$output" in
  *"source representation/ownership/lifecycle contract ok"*) ;;
  *) printf 'compiler output skipped canonical source audit: %s\n' "$output" >&2; exit 1 ;;
esac
case "$output" in
  *".s"*"bytes"*) ;;
  *) printf 'compiler output did not identify assembly artifact: %s\n' "$output" >&2; exit 1 ;;
esac
printf 'compiler output contract: ok\n'
