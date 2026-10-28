#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "%s\n" "$status" > /workspace/scratch/npm-decoder-production-draft/check.exit' EXIT
source /workspace/quench-build-env.sh
export CARGO_TARGET_DIR=/workspace/quench/target/npm-file-url CARGO_INCREMENTAL=0
export PKG_CONFIG_PATH=/workspace/shared/icu78-node/lib/pkgconfig
cd /workspace/scratch/quench-npm-file-url
cargo check --workspace --all-targets > /workspace/scratch/npm-decoder-production-draft/check.log 2>&1
