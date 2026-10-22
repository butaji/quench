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
