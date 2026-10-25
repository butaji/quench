# Cold entry compile release (TOD-123 / TOD-197)

Host: x86_64 Linux 6.18.44 VM. 4 vCPUs (Intel Xeon @ 2.10GHz), 16 GB RAM, no cgroup memory limit.
Binary: static x86_64 production build, BOLTed with the r48 profile (`qn-k`). The
`toggles.patch` env toggles were measured on that one binary.
Policy: `QUENCH_MEMORY_POLICY=memory-first`. RSS is `rss-exec --evict` max RSS (KiB).

## What the worst run was

Board b70 (r48) shows each no-JIT row's worst run as its first sample. Example: Richards
2696 vs median 2396 KiB. That sample is the board's cold run. A new fixture text misses the
compile cache, so the entry compiles, is stored, and is decoded back. `peakbt.py` stops the
process at its high-water mark: the peak is at the end of the entry's compilation, just
before the post-entry release in `commonjs::load_with_resolution`. In the same run, strace
shows no release between `release_before_entry_compile` and the cache store. The
code-generation release was held by its 100 ms interval.

## Toggles (cold runs: `coldsweep.sh`, a fresh text per run, configs interleaved, 9 runs)

- K1: release read-only code and data after `Host::keep_compiled_program`.
- K2: drop replayable records before decoding the kept form.
- K3: no interval on compile-phase releases.

| Fixture   | Base median/max | K1        | K2        | K3        | K1+K2+K3  |
|-----------|-----------------|-----------|-----------|-----------|-----------|
| Richards  | 2700/2880       | 2448/2640 | 2700/2704 | 2700/2824 | 2444/2580 |
| Crypto    | 3060/3184       | 2928/2928 | 3184/3268 | 2932/2932 | 2928/2928 |
| RayTrace  | 2852/2852       | 2784/2788 | 2852/2884 | 2848/2852 | 2636/2640 |
| DeltaBlue | 2880/2928       | 2604/2704 | 2880/2880 | 2880/2880 | 2496/2620 |

K2 doesn't help, so it was dropped. `k1-k3-cold-warm.txt` has the K1 vs K1+K3 cold runs
for seven fixtures, and K3 on warm runs (sweep.sh, 7 runs). K3 cuts warm runs too:
Richards 2188 -> 2052, DeltaBlue 2536 -> 2440, Crypto 2604 -> 2548.

## Landed form

- `resident_pages::after_program_kept` (K1).
- Every memory-first compile-phase release now uses one interval,
  `MEMORY_FIRST_COMPILE_RELEASE_INTERVAL` (5 ms). This is the bounded form of K3: with no
  interval at all, an npm run's back-to-back decodes release after each one. The throughput
  policy keeps `LOAD_RELEASE_INTERVAL`.
- Confirmed on the boards that follow (see TOD-123).

## Encoding release (r49 build plus `CompilePhase::Encoding`, `qn-e`)

After the kept-program release, the cold Crypto peak moved to `encode_cached_program`
(`peakbt.py`). Anon pages rise about 350 KiB for the program plus its encoding, beside
1.2 MB of code faulted since the code-generation release: the rest of code generation,
atom interning and the encoder. A release as encoding starts (`CompilePhase::Encoding`)
takes those pages out of the peak. Cold runs, an env toggle disabling it on the same
binary, 11 interleaved runs (a quiet rerun), median/max KiB:

| Fixture   | Without   | With      |
|-----------|-----------|-----------|
| Crypto    | 2832/2872 | 2572/2776 |
| Richards  | 2408/2412 | 2220/2412 |
| DeltaBlue | 2580/2752 | 2460/2788 |
| RayTrace  | 2584/2740 | 2556/2596 |

In the first 9-run pass, NavierStokes went 3648 -> 3684 median and RegExp and EarleyBoyer
were unchanged.
