#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=${1:-"$root/tests/node/test/parallel"}
exec cargo run --quiet --manifest-path "$root/Cargo.toml" -p quench-node -- --test-dir "$dir"
