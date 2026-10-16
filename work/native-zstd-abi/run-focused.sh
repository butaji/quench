#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
trap 'printf "%s\n" "$?" > work/native-zstd-abi/focused.exit' EXIT
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel > work/native-zstd-abi/paired-final-build.log 2>&1
target/iteration/quench-node --no-warnings --expose-internals --experimental-stream-iter work/native-zstd-abi/oracle.cjs > work/native-zstd-abi/candidate.log 2>&1
diff -u work/native-zstd-abi/oracle.log work/native-zstd-abi/candidate.log > work/native-zstd-abi/oracle-diff.log
cargo test --profile iteration -p quench-node-test --test child_entry native_zstd_streaming_frames_dictionaries_pledges_and_errors_match_node -j2 -- --test-threads=1 --exact > work/native-zstd-abi/targeted.log 2>&1
