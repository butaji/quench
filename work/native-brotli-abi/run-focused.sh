#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
trap 'printf "%s\n" "$?" > work/native-brotli-abi/focused.exit' EXIT
cargo build --profile iteration -p quench-node -p quench-node-test --bin quench-node --bin run-parallel > work/native-brotli-abi/paired-final-build.log 2>&1
target/iteration/quench-node --no-warnings --expose-internals --experimental-stream-iter work/native-brotli-abi/oracle.cjs > work/native-brotli-abi/candidate.log 2>&1
diff -u work/native-brotli-abi/oracle.log work/native-brotli-abi/candidate.log > work/native-brotli-abi/oracle-diff.log
cargo test --profile iteration -p quench-node-test --test child_entry native_brotli_streaming_bytes_dictionaries_and_errors_match_node -j2 -- --test-threads=1 --exact > work/native-brotli-abi/targeted.log 2>&1
