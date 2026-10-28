#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
cargo check --profile iteration -p quench-runtime -p quench-node --lib -j1 > work/sdk-primitives-policy/check.log 2>&1
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel -j1 > work/sdk-primitives-policy/paired-build.log 2>&1
for case in error_source_positions_preserve_captured_compiler_metadata_without_guest_hooks native_protocol_constants_derive_from_configured_sdk_headers_and_preserve_descriptors native_caller_location_uses_original_activations_independently_of_error_hooks filesystem_glob_policy_defers_promises_dependencies_and_preserves_data_descriptors; do
 cargo test --profile iteration -p quench-node-test --test child_entry "$case" -j1 -- --test-threads=1 > "work/sdk-primitives-policy/focused-$case.log" 2>&1
done
cargo test -p quench-runtime --lib -j1 > work/sdk-primitives-policy/core.log 2>&1
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/sdk-primitives-policy/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/sdk-primitives-policy/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/sdk-primitives-policy/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/sdk-primitives-policy/workspace.log 2>&1
