set -u
S=/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad
B=$S/qb6.static/quench-node; O=$S/bolt3
cd /home/user/quench
export QUENCH_COMPILE_CACHE= GLIBC_TUNABLES=glibc.malloc.tcache_count=0:glibc.malloc.top_pad=0
mkdir -p $O/nojit $O/jit; rm -f $O/nojit/* $O/jit/*
run() { m=$1; f=$2; a=""; [ $m = jit ] && a="--jit"; valgrind --tool=callgrind --dump-instr=yes --dump-line=no --demangle=no --dump-before=quench_entry_compiled --callgrind-out-file=$O/$m/$(basename $f).out $B $a $f >/dev/null 2>&1 || true; }
export -f run; export O B
for m in nojit jit; do
  run $m $S/order/empty.cjs
  ls quench-bench/deegen-curriculum/*.js quench-bench/micros/*.js $S/order/t262/*.js | xargs -P4 -I{} bash -c "run $m {}"
done
ls $O/nojit | head -5; ls $O/nojit | wc -l; ls $O/jit | wc -l
