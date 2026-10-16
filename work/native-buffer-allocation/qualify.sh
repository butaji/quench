#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
cargo test -p quench-runtime --lib -j2 > work/native-buffer-allocation/core-accounting.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j2 > work/native-buffer-allocation/accounted-paired-build.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j2 -- --test-threads=1 > work/native-buffer-allocation/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/native-buffer-allocation/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j2 -- --test-threads=1 > work/native-buffer-allocation/shared.log 2>&1
cargo check --workspace --all-targets -j2 > work/native-buffer-allocation/workspace.log 2>&1
