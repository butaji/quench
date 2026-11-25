# Data-structure suite (Gemini 4 Neon)

The suite from https://ben-chmarks.com/gemini-neon/: 9 structures (B-tree, doubly linked
list, hash map, binary heap, radix tree, red-black tree, R-tree, skip list, union-find),
10 cases each, with upstream's canonical Node and C drivers and the gemini-4-neon_high
solutions. Upstream publishes no license, so `fetch.sh` downloads it into `suite/` (ignored).
Quench runs the Node solution, the same source Node runs. Linear: TOD-2275.

    ./fetch.sh                                  # suite/, then build each suite/*/c/driver:
    for d in suite/*/c; do (cd $d && cc -std=c17 -O3 -march=native -flto -o driver *.c); done
    cc -O2 -o rss_exec rss_exec.c
    ./run.py --scale 0.1 --quench ../../target/experiment/quench   # wall time + peak RSS
    ./ir.py --ds unionfind --cases chain --n 20000                 # Ir per op, timed region
    ./cgself.py /tmp/cg.out.2 ...                                  # self Ir per function

`run.py`: one process per (structure, case) like upstream, median of the timed reps, geometric
mean of the 10 cases per structure, peak RSS through `rss_exec` (wait4), checksum and verify
compared with C. Upstream sizes are `SIZES`; `--scale` multiplies them.

`ir.py`: callgrind Ir inside the drivers' timed regions only. Quench marks them with
`process.__irMark()` (QUENCH_DEV=ir-mark, a driver copy `driver.ir.mjs`), C with
`--dump-before=*clock_gettime*` (built -march=x86-64-v3, as valgrind traps on AVX-512).

Target (user, 2026-11-25): in no-jit, b-jit and default, time <= 1/1.5 and peak RSS <= 0.5x
of the better of Node and Bun on every structure; C is ground truth.
