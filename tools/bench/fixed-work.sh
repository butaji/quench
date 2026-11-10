#!/bin/bash
# Fixed-work instruction counts for one V8-v7 benchmark body under callgrind.
# usage: tools/bench/fixed-work.sh BIN BENCH RUNS [VAR=value ...]
# Prints the callgrind "Collected" line (instructions) for RUNS runs of the benchmark body.
set -e
BIN=$1; B=$2; N=$3; shift 3
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
D=$ROOT/quench-bench/js-engine-benchmark/v8-v7
SCRIPT=$(mktemp --suffix=.js)
OUT=$(mktemp --suffix=.cg)
trap 'rm -f "$SCRIPT" "$OUT"' EXIT
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
env "$@" valgrind --tool=callgrind --callgrind-out-file="$OUT" "$BIN" "$SCRIPT" 2>&1 | grep -E "Collected|done"
