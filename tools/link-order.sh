#!/usr/bin/env bash
# Trains the dynamic x86_64 link order (crates/quench/link/x86_64-linux.order) the
# experiment build links with: every function the layout corpus executes under --no-jit and
# in default mode (so the compiler's code is packed too),
# ordered by how many corpus programs execute it, then by exactly which, then first
# appearance (symbol-order --sets). Each program is profiled twice, with an empty compile
# cache (the parser, analysis, code generation and Wasm validation run) and warm (the cache
# decodes), so a run that compiles as it goes and one that decodes both find their code packed.
#
# usage: tools/link-order.sh [BINARY]   (default: target/experiment/quench)
#
# Corpus: the interpreter curriculum, the numbered micros, Node library modules compiled
# without running, tools/layout-corpus/rt-*.js and npm-*.js (packages from its package-lock.json,
# installed with `npm ci`), and the Wasm kernels. The scored engine suites are never run.
# Requires valgrind and npm with registry access; takes about 15-30 minutes on 4 cores. Rebuild
# afterwards (the order is a link input) and check tools/check-link-order.sh stays near 100%.
set -euo pipefail
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"
binary="$(realpath "${1:-target/experiment/quench}")"
work="${CARGO_TARGET_DIR:-$root_dir/target}/link-order"
corpus="$work/corpus"
profiles="$work/profiles"
packages="$work/packages"
rm -rf "$corpus" "$profiles" "$work/homes"
mkdir -p "$corpus" "$profiles" "$packages"

cargo build --profile experiment -p quench-symbols --bin symbol-order >/dev/null

printf '{"type": "commonjs"}\n' >"$corpus/package.json"
printf 'console.log(1)\n' >"$corpus/000-print.cjs"
cp quench-bench/deegen-curriculum/[0-9]*.js "$corpus/"
cp tools/layout-corpus/rt-*.js tools/layout-corpus/npm-*.js "$corpus/"
cp quench-bench/micros/payload.js "$corpus/micro-payload.js"
for micro in quench-bench/micros/[0-9]*.js; do cp "$micro" "$corpus/micro-$(basename "$micro")"; done
index=0
for module in path _http_server _http_outgoing repl buffer fs net internal/modules/esm/loader \
    internal/modules/cjs/loader internal/util/inspect internal/readline/interface internal/http2/core \
    internal/url internal/crypto/webcrypto internal/crypto/keys; do
    index=$((index + 1))
    [ -f "tests/node/lib/$module.js" ] || continue
    { printf 'function __compileOnly() {\n'; cat "tests/node/lib/$module.js"; printf '\n}\nconsole.log(typeof __compileOnly);\n'; } \
        >"$corpus/$(printf 'c-compile-%02d.js' "$index")"
done
cp tools/layout-corpus/package.json tools/layout-corpus/package-lock.json "$packages/"
(cd "$packages" && npm ci --ignore-scripts --silent)

export binary corpus profiles work
export QUENCH_LAYOUT_NODE_MODULES="$packages/node_modules"
profile_one() {
    local program=$1 script="$corpus/$1" extra=() mode
    [ "$program" = wasm-kernels ] && script="$PWD/quench-bench/wasm-micros/kernels.cjs" extra=(3)
    for mode in no-jit default; do
        local home="$work/homes/$mode-$program" flags=(--no-jit)
        [ "$mode" = default ] && flags=()
        mkdir -p "$home"
        for state in cold warm; do
            (cd "$corpus" && env -i PATH="$PATH" HOME="$home" QUENCH_LAYOUT_NODE_MODULES="$QUENCH_LAYOUT_NODE_MODULES" \
                timeout 900 valgrind --tool=cachegrind --cache-sim=no --demangle=no --smc-check=all \
                --cachegrind-out-file="$profiles/$mode-$state-$program.out" "$binary" "${flags[@]}" "$script" "${extra[@]}" >/dev/null 2>&1 || true)
        done
    done
}
export -f profile_one
{ (cd "$corpus" && ls -- *.js *.cjs); echo wasm-kernels; } | xargs -P "$(nproc)" -I{} bash -c 'profile_one {}'

target/experiment/symbol-order "$binary" "$work/hashed.order" --sets "$profiles"/*.out
# Crate hashes move with dependencies and features: the order names functions without them
# (tools/ordered-ld/ld.lld resolves them in each link).
sed -E 's/Cs[0-9A-Za-z]+_/Cs_/g; s/\.llvm\.[0-9]+$//' "$work/hashed.order" | awk '!seen[$0]++' >crates/quench/link/x86_64-linux.order
tools/check-link-order.sh "$binary" crates/quench/link/x86_64-linux.order 0
