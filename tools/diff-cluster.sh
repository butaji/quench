#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
prefix=${1:-}
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-10}
if [ -z "$prefix" ]; then
  echo "usage: $0 <fixture-prefix>" >&2
  exit 2
fi

temporary=$(mktemp -d "${TMPDIR:-/tmp}/quench-diff.XXXXXX")
previous="$temporary/previous"
current_binary="$root/target/debug/quench-node"
previous_binary="$previous/target/debug/quench-node"
cleanup() {
  git -C "$root" worktree remove --force "$previous" >/dev/null 2>&1 || true
  rmdir "$temporary" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

git -C "$root" worktree add --quiet --detach "$previous" HEAD^
cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
cargo build --quiet --manifest-path "$previous/Cargo.toml" -p quench-node

run_fixture() {
  binary=$1
  fixture=$2
  if command -v timeout >/dev/null 2>&1; then
    timeout "$timeout_seconds" "$binary" "$fixture"
  elif command -v gtimeout >/dev/null 2>&1; then
    gtimeout "$timeout_seconds" "$binary" "$fixture"
  else
    perl -e 'alarm shift; exec @ARGV' "$timeout_seconds" "$binary" "$fixture"
  fi
}

while IFS= read -r fixture; do
  relative=${fixture#"$root/"}
  old_fixture="$previous/$relative"
  old_status=fail
  new_status=fail
  run_fixture "$previous_binary" "$old_fixture" >/dev/null 2>&1 && old_status=pass || true
  run_fixture "$current_binary" "$fixture" >/dev/null 2>&1 && new_status=pass || true
  if [ "$old_status" != "$new_status" ]; then
    printf '%s: %s -> %s\n' "$relative" "$old_status" "$new_status"
  fi
done <<EOF
$(find "$root/tests/node/test/parallel" -type f \( -name "*${prefix}*.js" -o -name "*${prefix}*.mjs" \) | sort)
EOF
