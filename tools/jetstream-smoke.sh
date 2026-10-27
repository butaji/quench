#!/usr/bin/env bash
# Crash check of a quench-node build on a few JetStream tests, run cold and once each through
# JetStream's own driver (`quench-bench/src/bin/jetstream-smoke.rs`). Exits non-zero when any
# test crashes, hangs or prints no score. It measures nothing.
#
# usage: tools/jetstream-smoke.sh QUENCH_NODE [--jit] [TEST...]
#
# Needs the JetStream corpus: git submodule update --init quench-bench/jetstream
set -euo pipefail
cd "$(dirname "$0")/.."
quench=$(realpath "${1:?usage: tools/jetstream-smoke.sh QUENCH_NODE [--jit] [TEST...]}")
shift
cargo run --quiet --release -p quench-bench --bin jetstream-smoke -- "$quench" "$@"
