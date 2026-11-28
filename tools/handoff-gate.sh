#!/bin/bash
# handoff-gate.sh [--fast]: the integration gates every hand-off and every merge into integ runs
# (TOD-2445 4-hour review rules 3-7, 2026-11-28). Any FAIL line blocks the merge. The last
# line is the verdict; the exit status is 1 when any gate failed.
#   G5 type pass: cargo check of every layer crate; integ stays at 0 type errors.
#   G4 opcode round trip (tests/staged_decoding) passes, and the solver test count
#      (tests/aot_facts) is reported and may not drop below tools/gate-baseline.txt.
#   G3 one.js and richards run in no-jit, default and AOT (a compiled image): any panic,
#      non-zero exit or wrong output blocks.
#   G6 a closed-world gap reached at run time ("not linked into the compiled image", an
#      unproven entry) blocks, whatever the exit status.
#   G7 score order interpreter < default < AOT on richards, median of RUNS harness runs each,
#      with the run count and build type printed. Fixed-work Ir (tools/nojit-richards.sh) decides
#      a disputed order; a wall-clock score alone never passes a lattice change (rule 1).
# --fast skips G7 (G3 still runs every mode once).
# Env: PROFILE (default experiment), RUNS (default 3), WORK (default a fresh temp dir).
set -uo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd); cd "$REPO"
PROFILE=${PROFILE:-experiment}; RUNS=${RUNS:-3}; WORK=$(realpath -m "${WORK:-$(mktemp -d)}"); mkdir -p "$WORK"
FAST=0; [ "${1:-}" = "--fast" ] && FAST=1
FAILED=0
fail() { echo "FAIL $*"; FAILED=1; }
pass() { echo "PASS $*"; }
BUILD="profile=$PROFILE incremental=${CARGO_INCREMENTAL:-default}"

# G5: the type pass.
if cargo check -q -p quench-runtime -p quench-runtime-engine -p quench-bjit -p quench -p quench-bench > "$WORK/check.log" 2>&1; then
  pass "G5 cargo check: 0 type errors"
else
  fail "G5 cargo check: $(grep -c '^error' "$WORK/check.log") errors (see $WORK/check.log)"
fi

# G4: the opcode round trip, then the solver test count against the baseline.
if cargo test -q -p quench-runtime --features test-support --test staged_decoding > "$WORK/decoding.log" 2>&1; then
  pass "G4 opcode round trip (tests/staged_decoding)"
else
  fail "G4 opcode round trip (see $WORK/decoding.log)"
fi
cargo test -q -p quench-runtime --features test-support --test aot_facts > "$WORK/aot_facts.log" 2>&1
SOLVER=$(grep -oE '[0-9]+ passed' "$WORK/aot_facts.log" | tail -1 | awk '{print $1}'); SOLVER=${SOLVER:-0}
BASE=$(awk '/^solver_tests_passing/ {print $2}' tools/gate-baseline.txt 2>/dev/null); BASE=${BASE:-0}
if [ "$SOLVER" -ge "$BASE" ]; then pass "G4 solver tests passing: $SOLVER (baseline $BASE)"
else fail "G4 solver tests passing: $SOLVER, below baseline $BASE"; fi

# The binary under test.
if ! cargo build -q --profile "$PROFILE" -p quench > "$WORK/build.log" 2>&1; then
  fail "build of quench ($BUILD), see $WORK/build.log"; echo "VERDICT: BLOCKED"; exit 1
fi
Q="$REPO/target/$PROFILE/quench"
V="$REPO/quench-bench/js-engine-benchmark/v8-v7"
[ -f "$V/richards.js" ] || git submodule update --init --depth 1 quench-bench/js-engine-benchmark
echo 'console.log(1+1)' > "$WORK/one.js"
{ cat "$V/base.js" "$V/richards.js"
  echo 'var r=[];BenchmarkSuite.RunSuites({NotifyResult:function(n,s){r.push(n+": "+s)},NotifyError:function(n,e){r.push("ERR "+n+" "+e)},NotifyScore:function(s){r.push("Score "+s)}});console.log(r.join("\n"))'
} > "$WORK/richards.js"
E="env XDG_CACHE_HOME=$WORK/xdg QUENCH_STARTUP_SNAPSHOT=off"

# Compile both images once (AOT mode).
for prog in one richards; do
  if ! $E QUENCH_TRACE=build-dir "$Q" compile "$WORK/$prog.js" -o "$WORK/$prog.aot" > "$WORK/$prog.compile.log" 2>&1; then
    fail "G3 AOT compile of $prog (see $WORK/$prog.compile.log)"
  fi
done

# runs MODE PROG: the program's output, its exit status in $WORK/status.
run() {
  case "$1" in
    no-jit) $E "$Q" --no-jit "$WORK/$2.js" ;;
    default) $E "$Q" "$WORK/$2.js" ;;
    aot) $E "$WORK/$2.aot" ;;
  esac
}
check_run() { # MODE PROG EXPECT-REGEX
  local out status; out=$(run "$1" "$2" 2>&1); status=$?
  echo "$out" > "$WORK/$2.$1.out"
  if echo "$out" | grep -qE 'panicked|not linked into the compiled image|unproven|SIGSEGV|Segmentation'; then
    fail "G3/G6 $2 $1: panic or closed-world gap: $(echo "$out" | grep -E 'panicked|not linked|unproven|SIG|Segm' | head -1)"
  elif [ $status -ne 0 ]; then fail "G3 $2 $1: exit $status"
  elif ! echo "$out" | grep -qE "$3"; then fail "G3 $2 $1: wrong output (see $WORK/$2.$1.out)"
  else pass "G3 $2 $1"; fi
}
for mode in no-jit default aot; do
  check_run $mode one '^2$'
  check_run $mode richards '^Score [0-9.]+$'
done

# G7: the score order on richards.
if [ $FAST -eq 0 ]; then
  median() { sort -n | awk '{a[NR]=$1} END {print (NR%2 ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2)}'; }
  declare -A SCORE
  for mode in no-jit default aot; do
    SCORE[$mode]=$(for i in $(seq "$RUNS"); do run $mode richards 2>/dev/null | grep -oE '^Score [0-9.]+' | awk '{print $2}'; done | median)
  done
  ORDER="no-jit ${SCORE[no-jit]} < default ${SCORE[default]} < AOT ${SCORE[aot]} (median of $RUNS runs, $BUILD)"
  if awk -v a="${SCORE[no-jit]:-0}" -v b="${SCORE[default]:-0}" -v c="${SCORE[aot]:-0}" 'BEGIN {exit !(a < b && b < c)}'; then
    pass "G7 score order: $ORDER"
  else fail "G7 score order: $ORDER"; fi
fi

echo "files in $WORK"
if [ $FAILED -eq 0 ]; then echo "VERDICT: PASS"; else echo "VERDICT: BLOCKED"; exit 1; fi
