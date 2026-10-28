#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
trap 'printf "%s\n" "$?" > work/native-brotli-abi/checks.exit' EXIT
cargo test -p quench-runtime --lib -j2 > work/native-brotli-abi/core-default.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j2 -- --test-threads=1 > work/native-brotli-abi/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/native-brotli-abi/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j2 -- --test-threads=1 > work/native-brotli-abi/shared.log 2>&1
cargo check --workspace --all-targets -j2 > work/native-brotli-abi/workspace.log 2>&1
