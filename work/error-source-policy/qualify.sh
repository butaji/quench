#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
cargo check --profile iteration -p quench-runtime -p quench-node --lib -j1 > work/error-source-policy/check.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/error-source-policy/paired-build.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry error_source_positions_preserve_captured_compiler_metadata_without_guest_hooks -j1 -- --test-threads=1 > work/error-source-policy/focused.log 2>&1
cargo test -p quench-runtime --lib -j1 > work/error-source-policy/core.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/error-source-policy/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/error-source-policy/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/error-source-policy/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/error-source-policy/workspace.log 2>&1
