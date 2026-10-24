# JetStream 3 lane fixtures, Quench no-JIT vs Bun default (BUN_JSC_useJIT=0)

Host: Linux x86_64, 4 vCPU, 16 GB (cloud container), 2026-10-21.
Quench: v2-cloud-jet at 8825f66a (release build, lane GC pacing).
Bun: /root/.bun/bin/bun 1.4.2, BUN_JSC_useJIT=0, **BUN_OPTIONS removed**.

The container's shell exports `BUN_OPTIONS=--smol`; earlier comparisons in this lane ran Bun in
that memory-saving mode, which lowered its scores (sha256, aes, js-tokens and lazy-collections
read as Quench wins there and are not). This run removes it.

One run per fixture and engine (`JetStreamDriver.js --no-prefetch --test=NAME` with the lane
prelude), max RSS from wait4. Score wins 7/34 (six Wasm fixtures; aes 1.01x is within noise),
max RSS wins 31/34 (losses: doxbee-promise, doxbee-async, babel-wtb), geomean Score ratio 0.524.

## Re-sweep at 0028c07f (same host and Bun reference)

Quench at 0028c07f (`scores-0028c07f.txt`), one run per fixture, against the Bun rows above.
Some runs overlapped a single-threaded build on this 4-vCPU host, which can only lower
Quench's scores. Geomean Score ratio 0.524 -> 0.656; Score wins 7 -> 11 (the six Wasm
fixtures, plus stanford-crypto-pbkdf2 1.04, hash-map 1.04, js-tokens 1.05, aes 1.08,
sha256 1.09, the smallest of them within run-to-run noise); max RSS wins 32/34 (losses:
doxbee-promise 50 vs 42 MB, babel-wtb 94 vs 93 MB), RSS geomean ratio 0.674. The lowest
Score ratios: argon2-wasm 0.11, source-map-wtb 0.16, web-ssr 0.22, doxbee-async 0.24.

## Re-sweep at `e787e2ec` (2026-10-22, quiet host, one round)

`scores-e787e2ec.txt`: geomean Score ratio vs Bun no-JIT **0.640** (34 tests), max RSS below
Bun on 32/34 (RSS geomean 0.665). The tests this session targeted moved: source-map-wtb
x3.11, babel-wtb x1.43, babylon-wtb x1.30, doxbee-promise x1.19, web-ssr x1.16,
typescript-lib x1.14, chai-wtb x1.11 against `0028c07f`.

The geomean is lower than 0.656 because one round is noisier than the effects: paired,
interleaved medians of 3 at `64da6ab8` vs `e787e2ec` show no regression on the tests that read
lowest here (json-parse-inspector 96.2 vs 96.9, gcc-loops-wasm 17.6 vs 17.9, sync-fs 6.74 vs
6.79), while this sweep's single json-parse round read 71.5 and that test spans 85-116 within
three rounds. A per-test single round on this host swings up to ~20%; geomeans of one-round
sweeps compare only within a few percent. Decisions use paired medians or callgrind counts.

## One-round sweep at `99450a62` (2026-10-22)

`scores-99450a62.txt`: quench `99450a62`, one round per test, same host and harness as above, against the Bun 1.4.2 `BUN_JSC_useJIT=0` rows of this directory. **Score geomean 0.701 of Bun no-JIT** (was 0.640 at `e787e2ec`, 0.656 at `0028c07f`); max RSS below Bun on 32/34 (RSS geomean 0.664; losses doxbee-promise ×1.12, babel-wtb ×1.02). Since `e787e2ec`: typescript-lib 0.39 → 0.55 of Bun (name-shared cache sites past the 16-bit site space, `81cfb3d2`/`64f5988c`, −12.1% instructions; natives the lane answers without frames, −1.5%), UniPoker 0.53 (lane `shift`, −7.1% instructions), babylon 0.67, espree 0.68, esprima 0.72, acorn 0.84. One round varies up to ~10% per test; the per-change decisions rest on callgrind counts and paired medians recorded in TOD-121.

## One-round sweep at `8e8faeb5` (2026-10-23)

`scores-8e8faeb5.txt`: quench `8e8faeb5` (v2-merged `cee13a309` merged in, quench-alloc linked by default), one round per test, same host (x86_64 Linux 6.18, 4 vCPU, 15 GiB) and harness, against the Bun 1.4.2 `BUN_JSC_useJIT=0` rows of this directory. **Score geomean 0.741 of Bun no-JIT** (was 0.701 at `99450a62`); max RSS below Bun on 30/34 (losses doxbee-promise 48.4 vs 41.3 MB, babel-wtb 93.1 vs 90.5, postcss-wtb 78.6 vs 74.6, doxbee-async 39.3 vs 38.2). At or above 1.2× Bun: the six Wasm tests except argon2 (sqlite3 1.73, zlib 3.00, HashSet 3.42, gcc-loops 4.50, quicksort 3.12, tsf 2.18); hash-map and aes 1.10, sha256 1.02. Furthest behind: argon2-wasm 0.11, doxbee-async 0.27, web-ssr 0.30, async-fs 0.36, json-stringify 0.39, validatorjs 0.39, doxbee-promise 0.41. Since `99450a62`: lane exits removed for `super` reads, string reads of absent names, array symbol keys, `typeof` globals and for-in helpers; lane entry follows jumps (lazy-collections −4.7% instructions); GC traces suspended frames by slice (doxbee-async −7.6%); await-free async calls pushed in the dispatch loop (doxbee-async −21% against the merge baseline); quench-alloc restored after the merge dropped it (bigint 8.28 → 13.87). One round varies up to ~10% per test.

## Re-baselined sweep at `711e270e` (2026-10-23, new host kernel)

The host moved from kernel fc-v140 to `6.18.44-fc-v153` (still Intel Xeon @ 2.10 GHz, 4 vCPU) and runs about 15% slower (the same binary scores 19.8 vs 23.2 on hash-map), so the Bun rows above no longer pair with new Quench runs. `scores-711e270e-host2.txt` pairs quench `711e270e` (v2-merged `d836ef374` merged in) with Bun 1.4.2 `BUN_JSC_useJIT=0` re-run on the same host, one round each, run alone. **Score geomean 0.817 of Bun no-JIT; max RSS geomean 1.44× smaller.** At or above 1.2× Bun on score: json-parse-inspector 1.26, hash-map 1.21 and the Wasm tests except argon2 (sqlite3 2.14, zlib 3.27, HashSet 3.91, gcc-loops 4.15, quicksort 3.17, tsf 2.55). At or above 1.2× smaller RSS: 22/34; RSS losses: postcss-wtb 0.93, doxbee-promise 0.87, babel-wtb 0.98. Furthest behind on score: argon2-wasm 0.23, doxbee-async 0.25, web-ssr 0.34, doxbee-promise 0.37, json-stringify 0.42, async-fs 0.45, sync-fs 0.47.

## Paired sweep at 740c3c6c1 (2026-10-23)

Quench (`quench-node`, release, HEAD 740c3c6c1) and Bun (`BUN_JSC_useJIT=0`) run back to
back on each test (`psweep.sh`: one `rsscmp.py` call per test), so host throughput drift
affects both sides of each ratio alike. Host: Linux 6.18.44-fc-v153, Xeon 2.10GHz, 4 vCPU.
Raw rows: `paired-740c3c6c-raw.txt`; ratios: `paired-740c3c6c-ratios.txt`.

Geomean score 0.790× Bun no-JIT, max RSS 1.53× smaller; 7/34 tests at ≥1.2× score
(stanford-crypto-sha256 and the six Wasm tests other than argon2), 25/34 at ≥1.2× smaller RSS.
The lowest score ratios are argon2-wasm 0.23, web-ssr 0.32, doxbee-async 0.34,
bigint-noble-ed25519 0.36, chai 0.38, validatorjs 0.39, doxbee-promise 0.41, async-fs 0.42.
RSS is above Bun on babel-wtb, postcss-wtb (0.95) and doxbee-promise (0.92).

## Paired sweep at 2e17be07a (2026-10-24)

Same method and host as the 740c3c6c1 sweep (`psweep.sh`, quench and Bun `BUN_JSC_useJIT=0`
back to back per test; Linux 6.18.44-fc-v153, Xeon 2.10GHz, 4 vCPU). Raw rows:
`paired-2e17be07-raw.txt`; ratios with the previous sweep's beside them:
`paired-2e17be07-ratios.txt`.

Geomean score 0.788× Bun no-JIT (was 0.790), max RSS 1.59× smaller (was 1.53); 6/34 tests at
≥1.2× score, 26/34 at ≥1.2× smaller RSS. Gains since 740c3c6c1: chai 0.38 → 0.56 and RSS
1.21 → 1.84, validatorjs 0.39 → 0.45, postcss 0.52 → 0.63 and RSS 0.95 → 1.12, babel RSS
0.95 → 1.09, doxbee-promise RSS 0.92 → 1.02, bigint 0.36 → 0.51, prettier 0.66 → 0.76,
esprima-next 0.73 → 0.89, json-stringify 0.47 → 0.54. The stanford-crypto tests and hash-map
fell (sha256 1.23 → 0.79, pbkdf2 1.10 → 0.70, aes 0.93 → 0.69, hash-map 1.08 → 0.88): the
merge of origin/v2-merged at 3464b1bb5 brought a ~30% slowdown on them (sha256 38.2 before
the merge, 26.7 at it, same host, paired medians); see the bisect note below.

Bisect: `git bisect run` over origin/v2-merged (good 1c4b797e2, scoring
stanford-crypto-sha256 ≥ 32) names b00f1cd5e (jit: share number bitwise lowering stage) as
the first bad commit. 282264598 restores int-tagged bitwise results; paired medians on the
same host then: sha256 27.6 → 39.5 (pre-regression 39.4), pbkdf2 28.0 → 38.1, aes 21.2 →
28.1, hash-map 23.3 → 24.6. The ratios above predate that fix.

## Paired sweep at 3a7eaaf0f (2026-10-24)

Same method and host as above. Raw rows `paired-3a7eaaf0-raw.txt`; ratios, with the
740c3c6c1 baseline beside them, `paired-3a7eaaf0-ratios.txt`.

Geomean score **0.845×** Bun no-JIT (740c3c6c1: 0.790), max RSS **1.58×** smaller (1.53);
8/34 tests at ≥1.2× score (the six Wasm tests other than argon2, stanford-crypto-sha256 1.22,
stanford-crypto-aes 1.33), 26/34 at ≥1.2× smaller RSS. Against the baseline: chai 0.38 → 0.57,
validatorjs 0.39 → 0.53, json-stringify 0.47 → 0.66, json-parse 0.49 → 0.53, postcss 0.52 →
0.68, bigint 0.36 → 0.57, sync-fs 0.50 → 0.58, aes 0.93 → 1.33, pbkdf2 1.10 → 1.16; RSS
babel 0.95 → 1.08, postcss 0.95 → 1.12, babel-minify 1.14 → 1.20, chai 1.21 → 1.68. js-tokens
(1.15 → 0.98) and babel (0.70 → 0.55) moved mostly with Bun's single-run scores (Bun js-tokens
20.7 → 26.0, babel 11.9 → 16.8); quench's own scores rose on both.

## Paired sweep at ae6e0837a (2026-10-24, perturbed)

Raw rows `paired-ae6e0837-raw-perturbed.txt`. Profiling and builds ran on the same 4-vCPU
host during this sweep, so quench's scores are low by an unknown amount (web-ssr 23.1 → 19.4
against the clean 3a7eaaf0f run with no change that could slow it); RSS is unaffected. Read it
for RSS only.

## Decoded text kept as UTF-8 (3bbc8be24)

Every web-tooling bundle the harness preloads (and the bundle it loads as a script) is mostly
ASCII with a handful of non-ASCII characters (babel.bundle.js: 5,616,858 characters, 10 of
them non-ASCII), so each was held as UTF-16 at twice its size. Decoded text of 64 KiB or more
now keeps its UTF-8 until a reader needs units; the compiler and JSON.parse read the UTF-8.

| test | max RSS before (KiB) | after | Bun no-JIT | ratio after |
|---|---|---|---|---|
| babel-wtb | 87684 | 83104 | 94676 | 1.14 |
| babel-minify-wtb | 62064 | 59656 | 71204 | 1.19 |
| postcss-wtb | 69368 | 64908 | 77528 | 1.19 |
| typescript-lib | 172704 | 172844 | 195928 | 1.13 |

babel-wtb's peak is now its bundle's compilation at load (78 MiB at 640 ms; steady state 72 →
65 MiB): the OXC tree of the 5.6 MB source is 36.6 MB of it (VmRSS by compile phase: start
16.5, parsed 53.1, semantic 61.2, emitted 66.1, tree dropped 29.7 MB). Scores are within the
host's noise (babel median 9.06 → 9.49 over 5 paired rounds; callgrind −2.6% instructions).
