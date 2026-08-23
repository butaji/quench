#!/bin/sh
# Compile the runtime and audit compiler-output contracts for hot and cold
# helpers. Source annotations are canonical intent; assembly proves cold code.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-"$root/target-value-assembly"}
cargo rustc --manifest-path "$root/Cargo.toml" -p quench-runtime --release --lib \
  --target-dir "$target_dir" -- --emit=asm >/dev/null
asm=$(find "$target_dir/release/deps" -maxdepth 1 -name 'quench_runtime-*.s' -type f -print | sort | tail -n 1)
[ -n "$asm" ] || { echo "no quench-runtime assembly output found" >&2; exit 1; }

source="$root/crates/quench-runtime/src/value.rs"
require_annotation() {
  operation=$1
  annotation=$2
  awk -v op="$operation" -v attr="$annotation" '
    index($0, attr) { annotated=1; next }
    $0 ~ "fn " op "[(]" { if (annotated) matched=1; exit(matched ? 0 : 1) }
    $0 ~ /^fn / { annotated=0 }
    END { exit(matched ? 0 : 1) }
  ' "$source" || {
    echo "missing $annotation contract for Value::$operation" >&2
    exit 1
  }
}

for operation in is_immediate is_primitive_tag is_nullish as_boolean from_small_integer as_small_integer number_bits primitive_tag_code checked_small_integer_add; do
  require_annotation "$operation" '#[inline(always)]'
done
for operation in throw_type_error throw_reference_error throw_syntax_error throw_range_error throw_uri_error; do
  require_annotation "$operation" '#[cold]'
  symbol=$(grep -Eo '__RNv[^[:space:]]*value[^[:space:]]*'"$operation" "$asm" | head -n 1 || true)
  [ -n "$symbol" ] || {
    echo "missing emitted call target for Value::$operation" >&2
    exit 1
  }
done

for operation in is_immediate is_primitive_tag is_nullish as_boolean from_small_integer as_small_integer number_bits primitive_tag_code checked_small_integer_add; do
  if grep -Eq '__RNv[^[:space:]]*value[^[:space:]]*'"$operation"'([.[:space:]]|$)' "$asm"; then
    echo "inline Value::$operation unexpectedly emitted as an out-of-line symbol" >&2
    exit 1
  fi
done
inline_count=9
cold_count=5
printf 'audited compiler output %s (%s bytes): inline/cold contracts ok (%s inline, %s cold)\n' \
  "$asm" "$(wc -c < "$asm" | tr -d ' ')" "$inline_count" "$cold_count"
