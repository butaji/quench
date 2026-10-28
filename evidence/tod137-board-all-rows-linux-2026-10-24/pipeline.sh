set -u
S=/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad
cd /home/user/quench
export QUENCH_COMPILE_CACHE=
RUSTFLAGS="-D warnings -C llvm-args=-align-all-functions=6 -C target-feature=+crt-static -C relocation-model=static" CARGO_TARGET_DIR=target/static cargo build -j4 --profile production --target x86_64-unknown-linux-gnu -p quench-node 2>&1 | grep -E "^error|Finished"
mkdir -p $S/qb9.static && cp target/static/x86_64-unknown-linux-gnu/production/quench-node $S/qb9.static/ || exit 1
unset QUENCH_COMPILE_CACHE
rm -rf $S/qb9
bash $S/train9.sh
CARGO_TARGET_DIR=target/plain cargo build -q --profile production -p quench-wasm-test --bin bolt-profile || exit 1
mkdir -p $S/bolt9/out
target/plain/production/bolt-profile $S/qb9.static/quench-node $S/bolt9/out --order crates/quench-node/link/x86_64-linux-static.order --dense 13dispatch_fast --dense run_frame_general_until $S/bolt9/nojit/*.out* $S/bolt9/jit/*.out* || exit 1
rm -f $S/bolt9/nojit/* $S/bolt9/jit/*
mkdir -p $S/qb9
(ulimit -v 12000000; llvm-bolt-20 $S/qb9.static/quench-node -o $S/qb9/quench-node -data=$S/bolt9/out/profile.fdata -reorder-blocks=ext-tsp -split-functions -split-all-cold -reorder-functions=cdsort -skip-funcs-file=$S/bolt9/out/skip.funcs -use-gnu-stack -jump-tables=move -no-threads > $S/qb9.bolt.log 2>&1)
grep "hot bytes" $S/qb9.bolt.log; ls -la $S/qb9/quench-node; echo PIPE-DONE
cd /home/user/quench
QUENCH_MEMORY_POLICY=memory-first node quench-bench/v8-v7-rss.cjs --quench $S/qb9/quench-node --qjs $S/qjs/qjs --runs 1 --rss-exec $S/rss-exec --out $S/board-qb9-warm.json > /dev/null 2>&1
QUENCH_MEMORY_POLICY=memory-first node quench-bench/v8-v7-rss.cjs --quench $S/qb9/quench-node --qjs $S/qjs/qjs --node /opt/node22/bin/node --bun /root/.bun/bin/bun --runs 5 --rss-exec $S/rss-exec --out $S/board-qb9.json > $S/board-qb9.txt 2>&1
echo BOARD-DONE
