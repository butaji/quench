#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "%s\n" "$status" > /workspace/scratch/npm-decoder-production-draft/focused-build.exit' EXIT
source /workspace/quench-build-env.sh
export CARGO_TARGET_DIR=/workspace/quench/target/npm-file-url CARGO_INCREMENTAL=0
export PKG_CONFIG_PATH=/workspace/shared/icu78-node/lib/pkgconfig
cd /workspace/scratch/quench-npm-file-url
cargo check --workspace --all-targets > /workspace/scratch/npm-decoder-production-draft/reduced-check.log 2>&1
cargo build -p quench-node --bin quench-node > /workspace/scratch/npm-decoder-production-draft/focused-build.log 2>&1
cp /workspace/quench/target/npm-file-url/debug/quench-node /workspace/scratch/npm-decoder-production-draft/quench-debug
