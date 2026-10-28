#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "%s\n" "$status" > /workspace/scratch/npm-decoder-production-draft/preflight.exit' EXIT
source /workspace/quench-build-env.sh
export CARGO_TARGET_DIR=/workspace/quench/target/npm-file-url CARGO_INCREMENTAL=0
export PKG_CONFIG_PATH=/workspace/shared/icu78-node-repro/lib/pkgconfig
cd /workspace/scratch/quench-npm-file-url
ICU_UC_NO_PKG_CONFIG=1 cargo check -p quench-node --all-targets > /workspace/scratch/npm-decoder-production-draft/no-icu-check.log 2>&1
cargo check --workspace --all-targets > /workspace/scratch/npm-decoder-production-draft/final-workspace-check.log 2>&1
cargo test -p quench-runtime --lib > /workspace/scratch/npm-decoder-production-draft/final-fast-core.log 2>&1
