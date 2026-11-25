#!/usr/bin/env bash
# Trains the order of the functions a compiled image's start executes
# (crates/quench/link/x86_64-linux-image.order), which `quench compile` lays ahead of the run
# order (build.rs): every function the image corpus executes, by how many corpus images
# execute it, then by exactly which, then first appearance (symbol-order --sets). An image
# starts on code the CLI's corpus never runs (the bootstrap restore, the native entry), which
# the run order leaves scattered among cold code.
#
# usage: tools/image-link-order.sh [BINARY]   (default: target/experiment/quench)
#
# Corpus: tools/image-layout-corpus, each program compiled with full support (the support a
# strict compile falls back to) and its symbols
# kept, run once with a line of input. The scored engine suites are never run. Requires
# valgrind; takes about a minute. Rebuild afterwards (the order is a link input).
set -euo pipefail
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"
binary="$(realpath "${1:-target/experiment/quench}")"
work="${CARGO_TARGET_DIR:-$root_dir/target}/image-link-order"
rm -rf "$work"
mkdir -p "$work/images" "$work/profiles"
cargo build --profile experiment -p quench-symbols --bin symbol-order >/dev/null
images=()
for program in tools/image-layout-corpus/*.js tools/image-layout-corpus/*.mjs; do
    name="$(basename "${program%.*}")"
    env -u NODE_OPTIONS -u NODE_PATH QUENCH_KEEP_SYMBOLS=1 RUSTC_BOOTSTRAP=1 "$binary" compile "$program" -o "$work/images/$name" --profile experiment --dynamic-source >/dev/null
    printf 'a line\n' | (cd "$work" && env -i PATH="$PATH" HOME="$HOME" valgrind --tool=cachegrind --cache-sim=no --demangle=no \
        --cachegrind-out-file="$work/profiles/$name.out" "$work/images/$name" x y >/dev/null 2>&1 || true)
    images+=("$work/images/$name")
done
target/experiment/symbol-order "${images[0]}" crates/quench/link/x86_64-linux-image.order --sets "$work"/profiles/*.out
tools/check-link-order.sh "${images[0]}" crates/quench/link/x86_64-linux-image.order 0
