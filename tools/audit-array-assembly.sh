#!/bin/sh
# Compile ArrayData and audit the dense access path for duplicate bounds checks.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-"$root/target-array-assembly"}
cargo rustc --manifest-path "$root/Cargo.toml" -p quench-runtime --release --lib \
  --target-dir "$target_dir" -- --emit=asm >/dev/null
asm=$(find "$target_dir/release/deps" -maxdepth 1 -name 'quench_runtime-*.s' -type f -print | sort | tail -n 1)
[ -n "$asm" ] || { echo "no quench-runtime assembly output found" >&2; exit 1; }
source="$root/crates/quench-runtime/src/value_array_data.rs"
# `dense_value_at` is intentionally written as one explicit storage-length
# check followed by the hole predicate and an unchecked load. Keep this
# source-level assertion because release LLVM may inline the method entirely.
awk '
  /pub\(crate\) fn dense_value_at\(&self, index: usize\)/ { in_fn=1; next }
  in_fn && /^    }/ { done=1; exit }
  in_fn && /self\.values\.len\(\)/ { lencheck=1 }
  in_fn && /self\.deleted\.get\(index\)/ { holes=1 }
  in_fn && /self\.values\.get_unchecked\(index\)/ { unchecked=1 }
  END { exit !(lencheck && holes && unchecked && done) }
' "$source" || { echo "dense_value_at no longer has the single-check unchecked-load contract" >&2; exit 1; }
printf 'audited ArrayData dense access in %s (%s bytes)\n' "$asm" "$(wc -c < "$asm" | tr -d ' ')"
