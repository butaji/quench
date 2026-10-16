#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
cargo check --profile iteration -p quench-runtime -p quench-node --lib -j1 > work/shadow-stack-isolation/final-check.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/shadow-stack-isolation/paired-build.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry shadow_realms_own_original_node_error_policy_default_hooks_and_private_fallbacks -j1 -- --test-threads=1 --exact > work/shadow-stack-isolation/focused.log 2>&1
cargo test -p quench-runtime --lib -j1 > work/shadow-stack-isolation/core.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/shadow-stack-isolation/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/shadow-stack-isolation/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/shadow-stack-isolation/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/shadow-stack-isolation/workspace.log 2>&1
