#!/usr/bin/env bash
# Profile-guided production build of quench-node.
#
# usage: tools/pgo-build.sh [PROFILE]   (default: production)
#
# 1. Builds an instrumented quench-node into target/pgo-instrumented.
# 2. Runs the training corpus: the interpreter curriculum micro programs and the Node
#    compatibility fixtures. The scored engine suites under quench-bench/js-engine-benchmark
#    are never run, so the profile describes general workloads, not benchmark identity.
# 3. Builds PROFILE with the merged profile; the binary lands at target/PROFILE/quench-node,
#    where the benchmark tooling reads it.
#
# Requires llvm-profdata from the toolchain's llvm-tools component
# (`rustup component add llvm-tools`).
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-production}"
target_dir="${CARGO_TARGET_DIR:-$root_dir/target}"
instrumented_dir="$target_dir/pgo-instrumented"
data_dir="$target_dir/pgo-data"
profdata="$target_dir/pgo-quench-node.profdata"
# Seconds each corpus program may run; fixtures that wait on the network or a child process
# only add their startup path.
per_program_timeout=10
# Each curriculum program runs this many times so interpreter loops, not the startup paths
# the many Node fixtures share, dominate the profile (3 repeats measured 3-7% less Score).
curriculum_repeats=20
# A curriculum program runs its micro workload (`microRun`) once at load, a few milliseconds
# against a startup the Node fixtures already train; a driver keeps calling it for this long
# in the same process, so each program's interpreter paths, not startup, carry its weight.
# Training with only the single load-time call left the PGO build slower than a plain build
# on NavierStokes, Crypto, Richards and Splay.
curriculum_budget_ms=6000

host="$(rustc -vV | sed -n 's/^host: //p')"
llvm_profdata="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-profdata"
if [[ ! -x "$llvm_profdata" ]]; then
    printf 'llvm-profdata not found; run: rustup component add llvm-tools\n' >&2
    exit 1
fi
run_limited() {
    if command -v timeout >/dev/null 2>&1; then
        timeout "$per_program_timeout" "$@"
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$per_program_timeout" "$@"
    else
        "$@"
    fi
}

# Link OpenSSL statically where its archives exist: a shared libcrypto/libssl is loaded and
# relocated at every start (~1 MB of startup RSS on x86_64 Linux) although only TLS uses it,
# while statically linked code costs memory only once it runs. Other hosts keep their link.
openssl_libdir="$(pkg-config --variable=libdir openssl 2>/dev/null || true)"
if [[ -n "$openssl_libdir" && -f "$openssl_libdir/libcrypto.a" && -f "$openssl_libdir/libssl.a" ]]; then
    export OPENSSL_STATIC=1 OPENSSL_NO_PKG_CONFIG=1 OPENSSL_LIB_DIR="$openssl_libdir"
    export OPENSSL_INCLUDE_DIR="$(pkg-config --variable=includedir openssl)"
fi

cd "$root_dir"
rm -rf "$data_dir"
mkdir -p "$data_dir"
# `--config` appends to the repository's configured rustflags instead of replacing them.
CARGO_TARGET_DIR="$instrumented_dir" cargo build --profile "$profile" -p quench-node \
    --config "build.rustflags=[\"-Cprofile-generate=$data_dir\"]"
instrumented="$instrumented_dir/$profile/quench-node"

# A fixed pool of profiles merged in place as programs exit keeps the raw data small.
export LLVM_PROFILE_FILE="$data_dir/quench-%4m.profraw"
drivers_dir="$target_dir/pgo-curriculum"
rm -rf "$drivers_dir"
mkdir -p "$drivers_dir"
for program in quench-bench/deegen-curriculum/*.js; do
    driver="$drivers_dir/$(basename "$program")"
    {
        cat "$program"
        printf '\nconst trainingEnd = Date.now() + %d;\nwhile (Date.now() < trainingEnd) microRun();\n' \
            "$curriculum_budget_ms"
    } >"$driver"
    run_limited "$instrumented" "$driver" >/dev/null 2>&1 || true
done
for _ in $(seq "$curriculum_repeats"); do
    run_limited "$instrumented" quench-bench/property-get.js >/dev/null 2>&1 || true
done
while IFS= read -r fixture; do
    (cd "$(dirname "$fixture")" && run_limited "$instrumented" "$(basename "$fixture")" \
        >/dev/null 2>&1) || true
done < <(find tests/node-compat -name '*.js' | sort)
unset LLVM_PROFILE_FILE

"$llvm_profdata" merge -o "$profdata" "$data_dir"/*.profraw
rm -rf "$data_dir"
CARGO_TARGET_DIR="$target_dir" cargo build --profile "$profile" -p quench-node \
    --config "build.rustflags=[\"-Cprofile-use=$profdata\"]"
printf 'PGO build: %s\n' "$target_dir/$profile/quench-node"
