# V8-v7 RSS: three consecutive passing boards (TOD-123, TOD-197 F1)

Goal: Quench's max RSS at least 20% below every other runtime's best run, on every V8-v7
bench, with and without JIT. Every board meets it on the median and the worst run, in all
16 cells (8 benches x no-JIT/--jit).

- **Binary:** `quench-node` from commit d610b4b83e. Static x86_64 production build,
  symbol order and BOLT profile r50, sha256 prefix 65ab2a807522a61a. One binary for all
  three boards.
- **Host:** x86_64 Linux 6.18.44 VM. 4 vCPUs (Intel Xeon @ 2.10GHz), 16 GB RAM, no
  cgroup memory limit.
- **Command:** `QUENCH_MEMORY_POLICY=memory-first quench-node quench-bench/v8-v7-rss.cjs`,
  with `--runs 5`, against QuickJS 2026-06-04, Node 26.10.0 (jitless and default) and
  Bun (no-JIT and default).
- **Measurement:** max RSS from `rss-exec --evict`, in KiB.
- **Cold first run kept:** every board writes its fixtures into a fresh temp directory,
  and the compile cache keys an entry by its file path, so each row's first sample compiles
  cold.
- **Checker:** `margin.py`. The target is 0.8 x the lowest run of any other runtime on
  that board; the margin is the target minus Quench's median (or worst) run.

| Board | Median fails | Worst-run fails | Min median margin | Min worst-run margin |
|---|---|---|---|---|
| 1 (r50) | 0 | 0 | 249 KiB | 49 KiB |
| 2 (r50-2) | 0 | 0 | 223 KiB | 163 KiB |
| 3 (r50-3) | 0 | 0 | 277 KiB | 65 KiB |

| Bench | Mode | Target (0.8 x best other) | Quench median / worst, boards 1-3 |
|---|---|---|---|
| richards | no-JIT | 2656-2672 | 2320/2428 · 2064/2320 · 2200/2328 |
| richards | --jit | 2656-2672 | 2064/2172 · 2064/2064 · 2064/2080 |
| deltablue | no-JIT | 2809-2908 | 2316/2332 · 2264/2388 · 2332/2440 |
| deltablue | --jit | 2809-2908 | 2396/2396 · 2392/2504 · 2392/2396 |
| crypto | no-JIT | 2777-2835 | 2484/2596 · 2528/2672 · 2524/2576 |
| crypto | --jit | 2777-2835 | 2528/2612 · 2612/2636 · 2520/2528 |
| raytrace | no-JIT | 2640-2681 | 2368/2460 · 2372/2416 · 2372/2380 |
| raytrace | --jit | 2640-2681 | 2388/2632 · 2372/2408 · 2372/2584 |
| earley-boyer | no-JIT | 13068-13308 | 5688/5688 · 5684/5688 · 5688/5976 |
| earley-boyer | --jit | 13068-13308 | 5760/5980 · 5820/6020 · 5752/5880 |
| regexp | no-JIT | 5315-5340 | 4820/4900 · 4816/4820 · 4820/4820 |
| regexp | --jit | 5315-5340 | 4944/4948 · 4944/4948 · 4944/4948 |
| splay | no-JIT | 49644-49856 | 31328/31332 · 31328/31332 · 31328/31332 |
| splay | --jit | 49644-49856 | 31340/31348 · 31344/31356 · 31344/31344 |
| navier-stokes | no-JIT | 3910-3961 | 3464/3672 · 3468/3468 · 3468/3468 |
| navier-stokes | --jit | 3910-3961 | 3604/3604 · 3604/3692 · 3604/3604 |

`board-r50-default.*` is the same binary under the default throughput policy against
QuickJS only (3 runs), for reference.
