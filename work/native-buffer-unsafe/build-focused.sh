#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
cargo check --profile iteration -p quench-node --lib -j1 > work/native-buffer-unsafe/check.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/native-buffer-unsafe/paired-build.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry native_buffer_ -j1 -- --test-threads=1 > work/native-buffer-unsafe/focused.log 2>&1
