#!/bin/sh
set -eu
export QUENCH_WASM_JIT_EMSCRIPTEN_MEMCPY_LEAF=0
exec /workspace/quench/target/production/quench-node "$@"
