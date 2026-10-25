#!/usr/bin/env bash
# Profile-guided production build of quench-node.
#
# usage: tools/pgo-build.sh [--static] [PROFILE]   (default: production)
#
# --static builds the static x86_64 Linux binary (C runtime linked in, static relocation
# model), the input of the BOLT layout step in docs/README.md: both the instrumented and the
# profile-guided build add `-Ctarget-feature=+crt-static -Crelocation-model=static` and build
# for the host triple, so the binary lands at target/<host>/PROFILE/quench-node. The
# repository's own rustflags (.cargo/config.toml) stay in force in every mode: each build
# appends its flags with `--config`, which cargo concatenates with the configured array.
#
# 1. Builds an instrumented quench-node into target/pgo-instrumented.
# 2. Runs the training corpus: the interpreter curriculum micro programs, the numbered
#    diagnostic micros and the Node compatibility fixtures. The scored engine suites under
#    quench-bench/js-engine-benchmark are never run, so the profile describes general
#    workloads, not benchmark identity.
# 3. Builds PROFILE with the merged profile; the binary lands at target/PROFILE/quench-node,
#    where the benchmark tooling reads it.
#
# Requires llvm-profdata from the toolchain's llvm-tools component
# (`rustup component add llvm-tools`).
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
static=false
if [[ "${1:-}" == "--static" ]]; then
    static=true
    shift
fi
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
# against a startup the Node fixtures already train; a driver keeps calling it for this much
# of the process's own CPU time, so each program's interpreter paths, not startup, carry its
# weight. Training with only the single load-time call left the PGO build slower than a plain
# build on NavierStokes, Crypto, Richards and Splay. The budget is CPU time, not wall time: a
# busy host then leaves each program's share of the profile as it is. Wall-time budgets
# trained one source revision into builds 20% apart on NavierStokes and 8% on Richards when
# other work shared the host.
curriculum_budget_ms=6000
# Optional representative React SSR samples can be added to the general profile with
# QUENCH_PGO_REACT_ROUNDS=N. Keep this off by default so production builds stay independent
# of npm fixture installation; the npm performance workflow can opt in when measuring React.
react_training_rounds="${QUENCH_PGO_REACT_ROUNDS:-0}"
# Neither the curriculum nor the Node fixtures serve HTTP for long, so the server path a web
# application runs (socket polling, request parsing, response composition and the node:http
# JavaScript around them) was laid out as cold code. The npm workload `http-echo-isolation`
# is a plain node:http server and client on one keep-alive connection, with no package
# dependencies; this many round trips (about fifteen seconds of the instrumented build) train it.
http_training_rounds="${QUENCH_PGO_HTTP_ROUNDS:-3000}"
# The curriculum stages interpreter mechanisms and runs no regular expression, string
# search or typed-memory work; the numbered diagnostic micros (quench-bench/micros, one
# operation family each, the same `microRun` protocol) cover those families, so a library
# path such as the RegExp matcher is not laid out as cold code. Each runs this long.
micros_budget_ms=1500

host="$(rustc -vV | sed -n 's/^host: //p')"
# RUSTFLAGS replaces every configured rustflags array, the repository's and the ones this
# script passes with `--config`, so a build under it would silently lose the profile.
if [[ -n "${RUSTFLAGS:-}" ]]; then
    printf 'unset RUSTFLAGS: it would replace the profile flags and the repository rustflags\n' >&2
    exit 1
fi
# The static mode's link flags, appended to both builds, and the target directory layout a
# `--target` build uses.
mode_flags=""
target_args=()
binary_subdir="$profile"
if [[ "$static" == true ]]; then
    mode_flags='"-Ctarget-feature=+crt-static", "-Crelocation-model=static", '
    target_args=(--target "$host")
    binary_subdir="$host/$profile"
fi
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
run_training_limited() {
    if command -v timeout >/dev/null 2>&1; then
        timeout 120 "$@"
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout 120 "$@"
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
CARGO_TARGET_DIR="$instrumented_dir" cargo build --profile "$profile" -p quench-node "${target_args[@]}" \
    --config "build.rustflags=[${mode_flags}\"-Cprofile-generate=$data_dir\"]"
instrumented="$instrumented_dir/$binary_subdir/quench-node"

# A fixed pool of profiles merged in place as programs exit keeps the raw data small.
export LLVM_PROFILE_FILE="$data_dir/quench-%4m.profraw"
drivers_dir="$target_dir/pgo-curriculum"
rm -rf "$drivers_dir"
mkdir -p "$drivers_dir"
train_micro_programs() {
    local corpus="$1" budget_ms="$2" program driver
    for program in "$corpus"/[0-9]*.js; do
        driver="$drivers_dir/$(basename "$corpus")-$(basename "$program")"
        {
            cat "$program"
            printf '\nconst trainingBudget = %d * 1000;\nconst trainingSpent = () => { const { user, system } = process.cpuUsage(); return user + system; };\nwhile (trainingSpent() < trainingBudget) microRun();\n' \
                "$budget_ms"
        } >"$driver"
        # A CPU-time budget can outlast the per-program wall limit on a busy host, and a
        # killed run writes no profile.
        run_training_limited "$instrumented" "$driver" >/dev/null 2>&1 || true
    done
}
train_micro_programs quench-bench/deegen-curriculum "$curriculum_budget_ms"
train_micro_programs quench-bench/micros "$micros_budget_ms"
for _ in $(seq "$curriculum_repeats"); do
    run_limited "$instrumented" quench-bench/property-get.js >/dev/null 2>&1 || true
done
while IFS= read -r fixture; do
    (cd "$(dirname "$fixture")" && run_limited "$instrumented" "$(basename "$fixture")" \
        >/dev/null 2>&1) || true
done < <(find tests/node-compat -name '*.js' | sort)
if (( http_training_rounds > 0 )); then
    QUENCH_BENCH_ROUNDS="$http_training_rounds" \
        run_training_limited "$instrumented" tests/npm-perf/workloads/http-echo-isolation.cjs >/dev/null 2>&1 || true
fi
if (( react_training_rounds > 0 )); then
    if [[ ! -f tests/npm-perf/node_modules/react/package.json ]]; then
        printf 'React PGO training requested; install tests/npm-perf dependencies with npm ci first\n' >&2
        exit 1
    fi
    NODE_ENV=production \
        QUENCH_BENCH_ROUNDS="$react_training_rounds" \
        QUENCH_BENCH_WARMUPS=250 \
        run_training_limited "$instrumented" tests/npm-perf/workloads/react-ssr.cjs >/dev/null
fi
unset LLVM_PROFILE_FILE

"$llvm_profdata" merge -o "$profdata" "$data_dir"/*.profraw
rm -rf "$data_dir"
# Indirect call promotion turns a profiled indirect call into a guarded direct call and may
# inline the callee there. The lane's dispatches are indirect tail calls between handlers;
# promoting one inlines a large handler (a frameless call) into a small one (a register
# move), whose every dispatch then pays the large one's prologue: Crypto and NavierStokes
# ran 38% more instructions once training made a call the move's most frequent successor.
CARGO_TARGET_DIR="$target_dir" cargo build --profile "$profile" -p quench-node "${target_args[@]}" \
    --config "build.rustflags=[${mode_flags}\"-Cprofile-use=$profdata\", \"-Cllvm-args=-disable-icp\"]"
binary="$target_dir/$binary_subdir/quench-node"
printf 'PGO build: %s\n' "$binary"
# Profile-guided inlining decides which lane handlers form their dispatch as a sibling jump
# (`tools/check-lane-tail-calls.sh`); a profile that keeps symbols reports the ones that do not.
if nm "$binary" 2>/dev/null | grep dispatch_fast >/dev/null; then
    printf 'Lane handlers dispatching with a call:\n'
    tools/check-lane-tail-calls.sh "$binary" || true
fi
