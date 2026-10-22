#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
nice -n 10 cargo test -p quench-runtime --lib -j1 > work/glob-policy/core.log 2>&1

cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/glob-policy/paired-build.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry fs_glob_uses_original_policy_and_real_dirent_types_across_all_three_apis -j1 -- --test-threads=1 --exact > work/glob-policy/focused.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/glob-policy/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/glob-policy/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/glob-policy/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/glob-policy/workspace.log 2>&1
