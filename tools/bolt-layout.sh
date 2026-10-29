#!/usr/bin/env bash
# Layout step of the qualification binary (docs/README.md, "Wasm execution modes and link
# order"): trains the static x86_64 link order and the BOLT profile on the binary being laid
# out, relinks with the order, and rewrites the relinked binary's block layout with llvm-bolt.
#
# usage: tools/bolt-layout.sh [--pgo PROFDATA] [PROFILE]   (default: production)
#
# With --pgo, every build here is the profile-guided build of tools/pgo-build.sh --static
# (the same flags, the given merged profile), so the order and the BOLT profile are trained on
# the PGO binary they lay out; tools/pgo-build.sh --static writes PROFDATA to
# target/pgo-quench-node.profdata. Without it, the plain static build is laid out.
#
# 1. Builds the static quench-node and the layout tools (symbol-order, bolt-profile).
# 2. Assembles the training corpus from the repository under target/layout-corpus: the
#    interpreter curriculum (quench-bench/deegen-curriculum), the numbered diagnostic micros
#    (quench-bench/micros), Node library modules compiled without running
#    (COMPILE_ONLY_MODULES), the Test262 tests tools/layout-corpus/test262.list names with
#    their harness, the package workloads tools/layout-corpus/npm-*.js on the packages its
#    package-lock.json pins (installed with `npm ci` under target/layout-packages), and the
#    Wasm kernels' start-up. The Wasm ESM corpus (docs/README.md) is not part of it: laid out
#    `--late` it added 0.98 MB of hot text and raised V8-v7 max RSS (pinned, 2026-10-29:
#    RegExp 4900 -> 5084 KiB, Richards 2288 -> 2364, DeltaBlue --jit 2684 -> 2732), failing
#    the target on two of three boards. The scored engine suites are never run. The
#    corpus used is written to target/layout-corpus/MANIFEST and printed at the end.
# 3. Profiles start-up (cachegrind) and the run phase after the entry's compilation
#    (callgrind, split at quench_entry_compiled), without and with --jit; the --jit profiles
#    of the micros and package workloads run under memory-first, whose JS tier compiles less.
# 4. Writes crates/quench-node/link/x86_64-linux-static.order, converts the profiles for
#    BOLT, relinks, and writes the bolted binary beside the relinked one (`-bolt`).
#
# Requires valgrind, npm with registry access, and llvm-bolt (BOLT 20; LLVM_BOLT names it
# when it is not on PATH). QUENCH_PGO_HOT_CALLSITE_THRESHOLD bounds a profile's hot-call-site
# inlining in the --pgo builds.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profdata=""
if [[ "${1:-}" == "--pgo" ]]; then
    profdata="$(realpath "$2")"
    shift 2
fi
profile="${1:-production}"
target_dir="${CARGO_TARGET_DIR:-$root_dir/target}"
host="$(rustc -vV | sed -n 's/^host: //p')"
llvm_bolt="${LLVM_BOLT:-$(command -v llvm-bolt || echo /usr/lib/llvm-20/bin/llvm-bolt)}"
jobs="$(nproc)"
corpus="$target_dir/layout-corpus"
startup="$target_dir/layout-startup"
run="$target_dir/layout-run"
order="$root_dir/crates/quench-node/link/x86_64-linux-static.order"
binary="$target_dir/$host/$profile/quench-node"
tools_dir="$target_dir/$profile"

# Seconds each profiled program may run under valgrind; a killed run writes no profile.
profile_timeout=600
# Node library modules the corpus compiles without running them: large modules whose parser,
# analysis and code generation paths a short script's own compilation does not reach.
COMPILE_ONLY_MODULES=(
    path _http_server _http_outgoing repl buffer fs net internal/modules/esm/loader
    internal/modules/cjs/loader internal/util/inspect internal/readline/interface
    internal/dtls/dtls internal/http2/core internal/url internal/crypto/webcrypto
    internal/crypto/keys
)
# The interpreter's dispatch stays whole: a program reaches whichever of its blocks its code
# needs (docs/README.md). The order keeps the lane dispatch whole; BOLT also keeps the general
# frame loop's blocks together.
ORDER_DENSE_FUNCTIONS=(--dense 13dispatch_fast)
BOLT_DENSE_FUNCTIONS=(--dense 13dispatch_fast --dense run_frame_general_until)
package_dir="$target_dir/layout-packages"

if [[ -n "${RUSTFLAGS:-}" ]]; then
    printf 'unset RUSTFLAGS: it would replace the layout build flags and the repository rustflags\n' >&2
    exit 1
fi
for tool in valgrind npm "$llvm_bolt"; do
    command -v "$tool" >/dev/null || { printf '%s not found\n' "$tool" >&2; exit 1; }
done

# The static build of tools/pgo-build.sh --static, profile-guided when a profile is given.
build_flags='"-Ctarget-feature=+crt-static", "-Crelocation-model=static"'
if [[ -n "$profdata" ]]; then
    build_flags+=", \"-Cprofile-use=$profdata\", \"-Cllvm-args=-disable-icp\""
    # The inlining cost a profile lets a hot call site's callee reach (LLVM's default 3000,
    # against 225 without a profile). Unset keeps LLVM's; a lower value bounds the hot code
    # the layout keeps resident.
    if [[ -n "${QUENCH_PGO_HOT_CALLSITE_THRESHOLD:-}" ]]; then
        build_flags+=", \"-Cllvm-args=-hot-callsite-threshold=$QUENCH_PGO_HOT_CALLSITE_THRESHOLD\""
    fi
fi
build() {
    CARGO_TARGET_DIR="$target_dir" cargo build --locked --profile "$profile" -p quench-node \
        --bin quench-node --target "$host" --config "build.rustflags=[$build_flags]"
}

cd "$root_dir"
CARGO_TARGET_DIR="$target_dir" cargo build --locked --profile "$profile" -p quench-wasm-test \
    --bin symbol-order --bin bolt-profile
build

# Test262 metadata of one test: its `flags`, `includes` and whether it is negative.
test262_field() {
    awk -v field="$2" '
        /^\/\*---/ { meta = 1; next }
        /^---\*\// { exit }
        !meta { next }
        $0 ~ "^" field ":" {
            line = $0; sub("^" field ":[ ]*", "", line)
            if (line != "") { gsub(/[\[\],]/, " ", line); print line; next }
            list = 1; next
        }
        list && /^[ ]+-/ { line = $0; sub(/^[ ]+-[ ]*/, "", line); print line; next }
        { list = 0 }
    ' "$1"
}

rm -rf "$corpus"
mkdir -p "$corpus"
# The repository's package.json declares ES modules; the corpus's scripts load as CommonJS, as a
# script outside a package scope does, so their profiles take the CommonJS entry path, which
# marks the entry's compilation (quench_entry_compiled) that splits the run-phase profiles.
printf '{"type": "commonjs"}\n' >"$corpus/package.json"
# The first start-up profile writes one line, as every program it lays out for does.
printf 'console.log(1)\n' >"$corpus/000-print.cjs"
cp quench-bench/deegen-curriculum/[0-9]*.js "$corpus/"
# The micros' shared payload, run bare: it stops at its first statement with a ReferenceError,
# so it trains the uncaught-error report.
cp quench-bench/micros/payload.js "$corpus/micro-payload.js"
for micro in quench-bench/micros/[0-9]*.js; do
    cp "$micro" "$corpus/micro-$(basename "$micro")"
done
index=0
for module in "${COMPILE_ONLY_MODULES[@]}"; do
    index=$((index + 1))
    {
        printf '// compile-only corpus entry from tests/node/lib/%s.js\nfunction __compileOnly() {\n' "$module"
        cat "tests/node/lib/$module.js"
        printf '\n}\nconsole.log(typeof __compileOnly);\n'
    } >"$corpus/$(printf 'c-compile-%02d.js' "$index")"
done
test262=tests/test262
index=0
while IFS= read -r test; do
    flags="$(test262_field "$test262/$test" flags)"
    {
        [[ " $flags " == *" onlyStrict "* ]] && printf '"use strict";\n'
        for harness in assert.js sta.js $(test262_field "$test262/$test" includes); do
            cat "$test262/harness/$harness"; echo
        done
        cat "$test262/$test"
    } >"$corpus/$(printf 'z-t%04d.js' "$index")"
    index=$((index + 1))
done <tools/layout-corpus/test262.list
rm -rf "$package_dir"
mkdir -p "$package_dir"
cp tools/layout-corpus/package.json tools/layout-corpus/package-lock.json "$package_dir/"
(cd "$package_dir" && npm ci --ignore-scripts --no-audit --no-fund --silent)
export QUENCH_LAYOUT_NODE_MODULES="$package_dir/node_modules"
cp tools/layout-corpus/npm-*.js "$corpus/"

# valgrind does not follow the static build's re-execution; give its allocator setting directly.
export GLIBC_TUNABLES=glibc.malloc.tcache_count=0:glibc.malloc.top_pad=0
rm -rf "$startup" "$run"
mkdir -p "$startup" "$run"
programs() { (cd "$corpus" && ls -- *.js *.cjs | grep -E "${1:-.}"); }
export binary corpus startup run profile_timeout
cachegrind() { # NAME ARGS... : start-up profile
    local name="$1"; shift
    timeout "$profile_timeout" valgrind --tool=cachegrind --cache-sim=no --demangle=no \
        --cachegrind-out-file="$startup/$name.out" "$binary" "$@" >/dev/null 2>&1 || true
}
callgrind() { # NAME ARGS... : run-phase profile, split at the entry's compilation
    local name="$1"; shift
    timeout "$profile_timeout" valgrind --tool=callgrind --dump-instr=yes --dump-line=no \
        --demangle=no --dump-before=quench_entry_compiled --callgrind-out-file="$run/$name.out" \
        "$binary" "$@" >/dev/null 2>&1 || true
}
export -f cachegrind callgrind
numbered='^[0-9]{3}\.js$'
packages='^(micro-[0-9]{3}|npm-[a-z-]+)\.js$'
programs | xargs -P "$jobs" -I{} bash -c 'cachegrind {} "$corpus/{}"'
for mode in no-jit jit; do
    jit=(); [[ "$mode" == jit ]] && jit=(--jit)
    cachegrind "wasm-kernels-$mode" "${jit[@]}" quench-bench/wasm-micros/kernels.cjs 3
done
programs "$numbered" | xargs -P "$jobs" -I{} bash -c 'cachegrind jit-{} --jit "$corpus/{}"'
programs "$packages" | xargs -P "$jobs" -I{} bash -c \
    'QUENCH_MEMORY_POLICY=memory-first cachegrind jitmf-{} --jit "$corpus/{}"'
programs "$numbered" | xargs -P "$jobs" -I{} bash -c 'callgrind jit-{} --jit "$corpus/{}"'
programs "$packages" | xargs -P "$jobs" -I{} bash -c \
    'QUENCH_MEMORY_POLICY=memory-first callgrind jitmf-{} --jit "$corpus/{}"'
programs | xargs -P "$jobs" -I{} bash -c 'callgrind {} "$corpus/{}"'

# Profiles go in priority order: start-up first, then the run phase without Test262, then
# Test262, so its tests add only the code the rest did not execute.
"$tools_dir/symbol-order" "$binary" "$order" --foreign-first "${ORDER_DENSE_FUNCTIONS[@]}" \
    "$startup/000-print.cjs.out" $(ls "$startup"/*.js.out) \
    "$startup/wasm-kernels-no-jit.out" "$startup/wasm-kernels-jit.out" \
    --run "$run/000-print.cjs.out" $(ls "$run"/*.js.out | grep -v /z-t) \
    --run $(ls "$run"/z-t*.js.out)
# The profiles' addresses are the profiled link's: convert them before relinking.
"$tools_dir/bolt-profile" "$binary" "$target_dir/layout-bolt" --order "$order" \
    "${BOLT_DENSE_FUNCTIONS[@]}" "$run"/*.out*
build
"$llvm_bolt" "$binary" -o "$binary-bolt" -data="$target_dir/layout-bolt/profile.fdata" \
    -reorder-blocks=ext-tsp -split-functions -split-all-cold -reorder-functions=cdsort \
    -skip-funcs-file="$target_dir/layout-bolt/skip.funcs" -use-gnu-stack -jump-tables=move
tools/check-link-order.sh "$binary" "$order"
{
    printf 'commit: %s\n' "$(git rev-parse HEAD)"
    printf 'pgo profile: %s\n' "${profdata:-none}"
    printf 'hot call-site threshold: %s\n' "${QUENCH_PGO_HOT_CALLSITE_THRESHOLD:-default}"
    printf 'curriculum: %d, micros: %d, compile-only modules: %d, test262: %d\n' \
        "$(ls "$corpus"/[0-9]*.js | wc -l)" "$(ls "$corpus"/micro-*.js | wc -l)" \
        "${#COMPILE_ONLY_MODULES[@]}" "$(ls "$corpus"/z-t*.js | wc -l)"
    printf 'packages: %d workloads on tools/layout-corpus/package-lock.json\n' \
        "$(ls "$corpus"/npm-*.js | wc -l)"
} | tee "$corpus/MANIFEST"
printf 'laid-out binary: %s-bolt\n' "$binary"
