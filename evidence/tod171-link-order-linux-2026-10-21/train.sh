set -e
S=/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad
O=$S/order
cd /home/user/quench
git checkout -q v2-gc
export QUENCH_COMPILE_CACHE=
cargo build -j4 --profile production -p quench-node 2>&1 | grep -E "^error|Finished"
cargo build -j4 --profile production -p quench-wasm-test --bin symbol-order 2>&1 | grep -E "^error|Finished"
mkdir -p $O/p0 && cp target/production/quench-node $O/p0/
cp target/production/symbol-order $O/
rm -f $O/train/*.out
run() { f=$1; valgrind --tool=cachegrind --cache-sim=no --demangle=no --cachegrind-out-file=$O/train/$(basename $f).out $O/p0/quench-node $f >/dev/null 2>&1 || true; }
export -f run; export O
run $O/empty.cjs
ls quench-bench/deegen-curriculum/*.js quench-bench/micros/*.js $O/t262/*.js | xargs -P4 -I{} bash -c 'run {}'
ls $O/train | wc -l
$O/symbol-order $O/p0/quench-node $O/x86_64-linux.order.new $O/train/empty.cjs.out $(ls $O/train/*.js.out)
wc -l crates/quench-node/link/x86_64-linux.order $O/x86_64-linux.order.new
