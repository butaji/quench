#!/bin/bash
# Fixed-work instruction counts for one V8-v7 benchmark under callgrind, with the JIT installing
# code: QUENCH_DEV=sync-compile makes a requested compile run inline (a background compile never
# lands under valgrind, so without it a JIT mode measures the interpreter only).
#
# usage: tools/bench/fixed-work.sh BIN BENCH RUNS [--no-jit] [DEV=name,name]
#   BIN    a prebuilt quench-node (no build here); build with `cargo build --profile experiment
#          -p quench-node` so every column compared comes from the same profile
#   RUNS   iterations of the benchmark body; BENCH one of the eight V8-v7 names
#   DEV    extra QUENCH_DEV names (e.g. js-lane-methods), added to sync-compile
# Prints: instructions (callgrind "Collected"), then `installed=<lane bodies + JS compiles>`.
# In a JIT mode it fails loudly when nothing installed.
set -e
BIN=$1; B=$2; N=$3; shift 3
JIT=1; DEV=sync-compile
for arg in "$@"; do
  case $arg in
    --no-jit) JIT=0 ;;
    DEV=*) DEV="$DEV,${arg#DEV=}" ;;
    *) echo "unknown option $arg" >&2; exit 2 ;;
  esac
done
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
D=$ROOT/quench-bench/js-engine-benchmark/v8-v7
SCRIPT=$(mktemp --suffix=.js)
OUT=$(mktemp --suffix=.cg)
LOG=${FIXED_WORK_LOG:-$(mktemp)}   # set FIXED_WORK_LOG to keep the run's stdout/stderr
trap 'rm -f "$SCRIPT" "$OUT"; [ -n "$FIXED_WORK_LOG" ] || rm -f "$LOG"' EXIT
cat > "$SCRIPT" <<JS
const fs=require('fs'),vm=require('vm');
globalThis.load=f=>vm.runInThisContext(fs.readFileSync('$D/'+f,'utf8'),f);
globalThis.print=console.log;
load('base.js'); load('$B.js');
var bm=BenchmarkSuite.suites[0].benchmarks[0];
if(bm.Setup) bm.Setup();
for (var i=0;i<$N;i++) bm.run();
if(bm.TearDown) bm.TearDown();
print('done');
JS
FLAGS=(); [ $JIT = 0 ] && FLAGS=(--no-jit)
QUENCH_DEV=$DEV QUENCH_TRACE=js-jit valgrind --tool=callgrind --callgrind-out-file="$OUT" "$BIN" "${FLAGS[@]}" "$SCRIPT" > "$LOG" 2>&1
grep -E "Collected" "$LOG"
INSTALLED=$(( $(grep -c '^QUENCH_LANE_BODY' "$LOG") + $(grep '^QUENCH_JS_JIT_STATS' "$LOG" | grep -o '"compile_successes":[0-9]*' | cut -d: -f2 | paste -sd+ - | bc) ))
echo "installed=$INSTALLED"
if [ $JIT = 1 ] && [ "$INSTALLED" -eq 0 ]; then echo "FAIL: JIT mode installed no code (is the binary built with sync-compile?)" >&2; exit 1; fi
