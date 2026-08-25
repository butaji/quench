#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$#" -ne 1 ]; then
  echo "usage: tools/run-node-tests.sh <fixture.js>" >&2
  exit 2
fi

fixture=$1
case "$fixture" in
  /*) path=$fixture ;;
  *) path="$root/$fixture" ;;
esac
if [ ! -f "$path" ]; then
  echo "error: fixture not found: $fixture" >&2
  exit 2
fi

cd "$root"
exec cargo run -q -p quench-node-test --bin run -- "$path"
