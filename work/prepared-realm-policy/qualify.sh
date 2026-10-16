#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
cargo check --profile iteration -p quench-runtime -p quench-node --lib -j1 > work/prepared-realm-policy/check.log 2>&1
cargo test -p quench-runtime --lib -j1 > work/prepared-realm-policy/core.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/prepared-realm-policy/paired-build.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry shadow_realms_own_original_node_error_policy_default_hooks_and_private_fallbacks -j1 -- --test-threads=1 --exact > work/prepared-realm-policy/focused.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/prepared-realm-policy/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/prepared-realm-policy/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/prepared-realm-policy/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/prepared-realm-policy/workspace.log 2>&1
