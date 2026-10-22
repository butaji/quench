#!/usr/bin/env bash
# All-eight instruction-count guard (decision 24): Callgrind instructions of a fixed-work
# run (Setup, N x run(), TearDown) and of N=0, for a base and a candidate quench-node.
# Usage: callgrind-guard.sh BASE CANDIDATE OUTDIR
set -euo pipefail
base=$1; cand=$2; out=$3; mkdir -p "$out"
# valgrind does not follow quench-node's one-time re-execution with glibc's thread-cache
# setting; giving the run that setting directly measures the allocator production uses.
export GLIBC_TUNABLES=${GLIBC_TUNABLES-glibc.malloc.tcache_count=0:glibc.malloc.top_pad=0}
suite=quench-bench/js-engine-benchmark/v8-v7
# A quarter of quench-bench's fixed-work plan (Crypto 4, DeltaBlue 40, EarleyBoyer 3,
# NavierStokes 5, RayTrace 10, RegExp 1, Richards 100, Splay 300), at least one run.
plan="crypto:1 deltablue:10 earley-boyer:1 navier-stokes:1 raytrace:3 regexp:1 richards:25 splay:75"
jobs=()
for entry in $plan; do
  fixture=${entry%%:*}; runs=${entry##*:}
  for n in 0 "$runs"; do
    script="$out/$fixture-$n.js"
    { cat "$suite/base.js"; echo; cat "$suite/$fixture.js"; echo
      cat <<JS
var __suite = BenchmarkSuite.suites[0];
for (var __b of __suite.benchmarks) { if (__b.Setup) __b.Setup(); }
for (var __i = 0; __i < $n; __i++) for (var __b of __suite.benchmarks) __b.run();
for (var __b of __suite.benchmarks) { if (__b.TearDown) __b.TearDown(); }
console.log("done $fixture $n");
JS
    } > "$script"
    for side in base cand; do jobs+=("$side $fixture $n $script"); done
  done
done
run() { set -- $1; bin=$base; [ "$1" = cand ] && bin=$cand
  ir=$(valgrind --tool=callgrind --callgrind-out-file=/dev/null "$bin" "$4" 2>&1 | awk '/refs:/{gsub(",","",$NF);print $NF}')
  echo "$1 $2 $3 $ir"; }
export -f run; export base cand
printf '%s\n' "${jobs[@]}" | xargs -P 4 -I{} bash -c 'run "{}"' > "$out/raw.txt"
python3 - "$out/raw.txt" "$plan" <<'PY'
import sys
rows={}
for line in open(sys.argv[1]):
    side,fx,n,ir=line.split(); rows[(side,fx,int(n))]=int(ir)
print(f"{'fixture':14}{'startup base':>15}{'cand':>15}{'per-run base':>15}{'cand':>15}{'delta':>9}")
for entry in sys.argv[2].split():
    fx,n=entry.split(':'); n=int(n)
    sb,sc=rows[('base',fx,0)],rows[('cand',fx,0)]
    pb=(rows[('base',fx,n)]-sb)/n; pc=(rows[('cand',fx,n)]-sc)/n
    print(f"{fx:14}{sb:15,}{sc:15,}{pb:15,.0f}{pc:15,.0f}{100*(pc-pb)/pb:8.2f}%")
PY
