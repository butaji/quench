# usage: mkbolt.sh NAME  -> builds static from the working tree, bolts with bolt2 profiles, output $S/NAME/quench-node
set -eu
S=/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad; O=$S/bolt2; N=$1
cd /home/user/quench
export QUENCH_COMPILE_CACHE=
RUSTFLAGS="-D warnings -C llvm-args=-align-all-functions=6 -C target-feature=+crt-static -C relocation-model=static" CARGO_TARGET_DIR=target/static cargo build -j4 --profile production --target x86_64-unknown-linux-gnu -p quench-node 2>&1 | grep -E "^error|Finished"
mkdir -p $S/$N.static $S/$N && cp target/static/x86_64-unknown-linux-gnu/production/quench-node $S/$N.static/
(ulimit -v 12000000; llvm-bolt-20 $S/$N.static/quench-node -o $S/$N/quench-node -data=$O/profile.fdata -reorder-blocks=ext-tsp -split-functions -split-all-cold -reorder-functions=user -function-order=$O/function.order -skip-funcs-file=$O/skip.funcs -use-gnu-stack -jump-tables=move -no-threads > $S/$N.bolt.log 2>&1)
grep "hot bytes" $S/$N.bolt.log; sync; ls -la $S/$N/quench-node
