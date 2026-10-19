# Compiled Wasm: small modules inline leaves; trivial loop phis (Linux x86_64, 2026-10-19)

Intel Xeon @ 2.10 GHz (family 6 model 207), 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits,
rustc 1.99.0. Static production `quench-node` (docs/README.md "Qualification builds"), base
`65c82c59b`.

## What V8 does that this did not

`node --perf-basic-prof` on CoreMark puts V8's time in the same four functions, in the same
shares, as `quench-node --jit` (35.7 / 24.2 / 23.5 / 14.8% against 36.9 / 24.1 / 23.6 /
14.5%): no single function lost, every one ran about 4% slower. Straight-line code matches
instruction for instruction (`crcu8`'s eight `cmov` steps are the same in both); what differs
is per call and per iteration: V8 inlines and unrolls, while each compiled Quench call keeps
its frame, its stack check, an interrupt check at entry and the caller's status test.
Compiled lowering kept every call (`leaf_budget` 0) since inlining with the interpreter's
512-operator budget slowed large modules: Wasm compiles eagerly and every copy is compiled
whether it runs or not.

## Change

* `inline::leaf_budget` takes the module's operator count in compiled mode: a module of at
  most `COMPILED_INLINING_MODULE_OPERATORS` (32,768) operators inlines leaves as the
  interpreter does; larger ones keep every call. The count is one pass over the bodies,
  taken only when lowering for compiled code. Tests of compiled calls themselves keep every
  call (`COMPILED_CALLS_KEPT_FOR_TEST`).
* `remove_constant_phis` (vendored Cranelift) removes a loop header's formal that only its
  entry value and its own back edge reach, also when the entry value is another block's
  formal (the trivial phi of Braun et al.); the stock pass kept it whenever that value was a
  formal taking several values, so a loop-invariant `idx & 0xffff` in CoreMark's list search
  was recomputed and reloaded from the stack every iteration.

Module operators (compiled lowering): CoreMark 3,759; JetStream quicksort 3,348, richards
3,636, argon2 12,863, zlib 26,214, Box2D 26,056 (inlined); TSF 57,048, gcc-loops 63,737,
HashSet 127,728, sqlite3 347,797, tfjs 146,218/188,088, dotnet 954,084/2,568,794 (calls kept).
Inlining grew compiled bytecode: CoreMark 6.4K → 18.3K, zlib 36K → 57.5K, quicksort 8.6K →
11.0K, gcc-loops 212K → 330K, TSF 290K → 582K. gcc-loops' compile with inlining took 948 ms
against 568 ms, which is why the size bound exists.

## CoreMark

Callgrind, fixed work (`evidence/wasm-jit-interrupt-checks-linux-2026-10-18/run-fixed.mjs`):
base 3,749.9M; trivial phis 3,742.9M (−0.19%); + inlining 3,603.5M (−3.9%). Budgets of 16 to
128 operators inline nothing in CoreMark.

Score, `run-js.mjs`, five interleaved rounds (Node v26.12.0 / base / inlining):
18,564 / 18,083 / 19,148; 18,562 / 17,599 / 19,633; 18,898 / 18,709 / 19,433; 18,745 / 18,216 /
18,980; 18,620 / 18,229 / 19,206.

## JetStream 3 Wasm fixtures

`evidence/wasm-jit-loops-and-addresses-linux-2026-10-18/jetstream-fixed.cjs`, task clock.
Modules above the bound compile exactly as before. The inlined ones:

| Fixture | Iterations | Base | Inlining |
| --- | --- | --- | --- |
| zlib | 8, three rounds of 3 runs | 3,256 / 3,342 / 3,294 ms | 2,913 / 3,034 / 2,849 ms |
| zlib | 1 (compile included), three rounds of 5 | 768 / 759 / 770 ms | 745 / 788 / 751 ms |
| quicksort | 20, three rounds of 3 | 1,330 / 1,336 / 1,375 ms | 1,362 / 1,398 / 1,313 ms |
| quicksort | 1, eight rounds of 5 (median) | 376.5 ms | 380.5 ms |
| richards | 1, three rounds of 5 | 2,368 / 2,529 / 2,474 ms | 2,387 / 2,435 / 2,382 ms |

Callgrind instructions rise for zlib (+5.27% at one iteration, +1.35% at three) and quicksort
(+2.80%, +3.29%) while their wall time falls or holds: the calls an instruction count barely
weighs (frames, checks, status tests) are what inlining removes. richards is unchanged
(+0.01%). argon2 through `quench-bench --suite jetstream3 --jit`, two runs of three rounds:
base scores 68.92/89.45 and 77.28/94.66, inlining 80.04/95.20 and 80.39/85.57. Box2D has no
harness here and is not in `quench-bench`'s corpus.

## Campaign (`coremark-11.csv`, 11 rounds)

`../wasm-coremark-rss-order-lazy-fs-linux-2026-10-19/run-coremark-evicted.sh` with
`ONLY='quench-node|node26|wasm3|wasmi'`; static `quench-node` of this change, bolted from that
evidence's profiles (`binaries.sha256`). Every engine scored lower than in that campaign's
session (Node's median 18,481 against 19,541): compare within a campaign only.

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node** | **2,745.0** | 2,499–2,788 | **3.71 MB** |
| No JIT | Wasmi v2 | 2,586.5 | 2,272–2,682 | 5.06 MB |
| No JIT | Wasm3 | 1,767.3 | 1,643–1,817 | 3.91 MB |
| JIT | Node v26.12.0 | 18,480.9 | 17,449–19,448 | 52.47 MB |
| JIT | quench-node --jit | 18,345.3 | 16,937–19,312 | 6.56 MB |

* JIT Score: 0.993x Node, ahead in 6 of 11 rounds (0.958x and 1 of 11 before).
* No-JIT max RSS: below Wasm3 in 11 of 11 rounds; JIT max RSS rose from 5.23 MB (inlined
  bodies compile larger), against Winch's 11.67 MB.

## Checks

Wasm testsuite: no JIT 67,124/67,124; JIT 66,952/66,952. Runtime lib 744/744.
