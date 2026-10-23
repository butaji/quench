#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
binary="$root/target/iteration/quench-node"

if [[ ! -x "$binary" ]]; then
  cargo build --quiet --profile iteration --manifest-path "$root/Cargo.toml" -p quench-node
fi

run_example() {
  local file="$1"
  echo "example=$file"
  "$binary" "$root/examples/$file"
}

run_example cli-summary.js
run_example crypto-file-summary.js
run_example http-loopback.js
run_example stream-summary.js
