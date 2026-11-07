#!/usr/bin/env bash
# Trains the profile every optimized x86-64 Linux build uses
# (crates/quench-node/pgo/x86_64-linux.profdata, applied by tools/rustc-pgo.sh).
#
# usage: tools/pgo-build.sh
#
# 1. Builds an instrumented release quench-node into target/pgo-instrumented.
# 2. Runs the training corpus: the interpreter curriculum micro programs, the numbered
#    diagnostic micros and the Node compatibility fixtures. The scored engine suites
#    (JetStream, quench-bench/js-engine-benchmark) are never run, so the profile describes
#    general workloads, not benchmark identity.
# 3. Merges the raw profiles into the committed profile; the next `cargo build --release`
#    uses it. Retrain when the profile's code has drifted (a rebuilt binary loses speed).
#
# Requires llvm-profdata from the toolchain's llvm-tools component
# (`rustup component add llvm-tools`).
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$root_dir/target}"
instrumented_dir="$target_dir/pgo-instrumented"
data_dir="$target_dir/pgo-data"
profdata="$root_dir/crates/quench-node/pgo/x86_64-linux.profdata"
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

cd "$root_dir"
rm -rf "$data_dir"
mkdir -p "$data_dir"
QUENCH_PGO_GENERATE="$data_dir" CARGO_TARGET_DIR="$instrumented_dir" cargo build --release -p quench-node --bin quench-node
instrumented="$instrumented_dir/release/quench-node"

# One raw profile per process: Darwin's runtime does not reliably merge concurrent
# writers into a shared pool file here (llvm-profdata then reports "too much profile data").
export LLVM_PROFILE_FILE="$data_dir/quench-%p.profraw"
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
        # killed run writes no profile. Each program also runs with --jit, so the Cranelift
        # compiler and the native code's runtime helpers are not laid out as cold code.
        run_training_limited "$instrumented" "$driver" >/dev/null 2>&1 || true
        run_training_limited "$instrumented" --jit "$driver" >/dev/null 2>&1 || true
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

# A training process the per-program wall limit kills writes a truncated profile, and one
# whose counters overflow writes a "too much profile data" one; both are still merged. The
# default failure mode rejects the run when the last of them is truncated, so ask for
# warnings instead of an abort and let a genuinely empty corpus fail below.
# Zero-count functions carry no information (a function without counts is optimized as
# without a profile), so the sparse profile keeps the committed file small.
mkdir -p "$(dirname "$profdata")"
"$llvm_profdata" merge --sparse --failure-mode=warn -o "$profdata" "$data_dir"/*.profraw
rm -rf "$data_dir"
printf 'PGO profile: %s\n' "$profdata"
