#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "%s\n" "$status" > /workspace/scratch/npm-decoder-production-draft/api-check.exit' EXIT
source /workspace/quench-build-env.sh
export PATH="/workspace/shared/node-v26.12.0-linux-x64/bin:$PATH"
export CARGO_TARGET_DIR=/workspace/quench/target/npm-file-url CARGO_INCREMENTAL=0
export PKG_CONFIG_PATH=/workspace/shared/icu78-node/lib/pkgconfig
export QUENCH_NODE_TEST_EXECUTABLE=/workspace/scratch/npm-decoder-production-draft/quench-debug
export QUENCH_NODE_LIBRARY_ROOT=/workspace/scratch/quench-npm-file-url/tests/node
cd /workspace/scratch/quench-npm-file-url
cargo test -p quench-node-test --test child_entry -- --test-threads=1 > /workspace/scratch/npm-decoder-production-draft/api-check.log 2>&1
