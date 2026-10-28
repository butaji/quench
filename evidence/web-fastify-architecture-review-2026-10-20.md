# Quench web-suite architecture review (fastify first)

Host: 4 vCPU cloud container, Linux 6.18, no-JIT rivals: Node v26.12 `--jitless`, Bun `BUN_JSC_useJIT=0`.

## Where fastify started (c9e12ed5, 5 s rounds, 500 connections)

| route | Quench | Node --jitless | Bun no-JIT |
|---|---|---|---|
| ping | 6870–6963 | 6045 | 5903 |
| query | 5483–6009 | 6120 | 5502 |
| body | 3000–3197 | 4539 | 4435 |
| peak RSS (ping+query+body) | 84.5 MB → **51 MB** after #1/#1b | 98 MB | 42 MB |

Ping was won; query was within noise of Node; body was 30% behind; RSS was 2× Bun.

## After this batch (3c2e9f3b, same host, same method; absolute rates are higher this run)

| route | Quench (2 rounds) | Node --jitless | Bun no-JIT |
|---|---|---|---|
| ping | 9014 / 8563 | 8017 | 6341 |
| query | 6770 / 7326 | 8471 | 6435 |
| body | 4363 / 4503 | 5835 | 5004 |
| peak RSS | 52.0 / 51.1 MB | 101.1 MB | 42.8 MB |

Ping beats both; query beats Bun and trails Node by ~17%; body trails Node by ~24% and Bun by ~11%;
peak RSS is half of Node's and 8 MB above Bun's.

## Findings

1. **Startup footprint and unbounded host bookkeeping make the RSS gap.** Fastify idles at 56 MB (49 MB anonymous) before the
   first request; 500 connections add 7–10 MB. Bun's whole peak is 42 MB. The JS heap is only 11.5 MB.
   - Rust `regex` automata built eagerly at RegExp construction: 9.9 MB, mostly two ajv-formats URI
     grammars (~1150-char case-insensitive patterns) the benchmark never executes. **Fixed: lazy byte
     backend** (idle 56.1 → 46.7 MB, body peak 66 → 56 MB).
   - Host-side request bookkeeping grew without bound under keep-alive load: every served
     response's host record (headers and body bytes) stayed until its socket closed, and every
     request and `nextTick` left an async-hooks trigger entry forever. **Fixed: response records
     hold a weak reference to the guest response object and are retired once a collection proves
     it dead; completed contexts drop their trigger.** Fastify ping+query+body peak 75 → 51 MB.
   - Compiled programs: 12.3 MB for 9,180 functions in 1,487 programs; only 1,729 functions (19%) ever
     run. Uncalled functions hold 5.2 MB of 392-byte records and bytecode, plus constants, positions,
     atom lists and source. The 1,280 `new Function` programs (fastify/ajv code generation) take 2.3 MB.
2. **Re-entry from native code into JavaScript is the throughput structure problem.** Micro-benchmarks
   (1M iterations, ms, Quench / Node --jitless): plain call 56/46, `forEach` callback 311/64,
   `emit` 1424/186, `Reflect.apply` 294/91. A native→JS call costs ~1,500 instructions: ~250 in the
   call_value layering, ~350 frame activation/deactivation, and the callee runs in the general loop
   (~860 instructions for a 4-instruction arrow body) because loop-free bodies shorter than
   `LANE_ENTRY_MIN_CALL_RUN` never enter the lane. Fastify does ~45 such entries per request (event
   emission, nextTick callbacks, stream callbacks, Function.prototype.call/apply through natives).
3. **Host API (rooted handles) cost.** Each `NativeContext` call roots its result in the generational
   root table and releases it on drop (~1K instructions per `emit`), named reads re-resolve through
   the generic `[[Get]]`, and EventEmitter `emit` allocates a fresh `"error"` string on every call.
4. **Uncached keyed access** (fixed in 3519b728): symbol-keyed reads walked chains every time;
   getters two prototypes up ran as nested host calls.

## Changes, ranked by expected effect

| # | change | lane | expected effect | status |
|---|---|---|---|---|
| 1 | Lazy regex byte backend | RSS | −9 MB fastify idle | **done** (c021de28) |
| 1b | Host records owned by guest objects (weakly), contexts retired on completion | RSS | −24 MB fastify peak, bounded growth | **done** (4edde0f0) |
| 3 | EventEmitter emit: one array-like pass for arguments, retained `"error"` name; dense CreateListFromArrayLike | Node/Score | emit −12.5%, body −0.9% | **done** (3c2e9f3b) |
| 2 | ~~Enter the lane at pc 0 for host entries~~ measured worse (forEach +3.3%, body +0.5%): lane entry costs as much as a short general run; the fixed cost is in the call layering and frame activation | Score | — | rejected |
| 2b | Slim native→JS call path: resolved target cache, one activation routine instead of six layers | Score | ~−700 instr per re-entry, ~3–4% fastify body; large on forEach/map/emit-heavy code | next |
| 4 | Lazy function compilation: keep the parent's scope maps and the function's span; compile the body on first call | RSS | −5–7 MB fastify idle, faster startup | design |
| 5 | Function record diet: move cold fields (parameter/eval/global-declaration data) behind one box | RSS | −1.5–2 MB fastify | planned |
| 6 | Scoped host handles (stack-allocated root scope, release by truncation) instead of the generational root table for callback-local values | Score/Node | ~−1K instr per host call | design |

## Correctness found on the way

- c84c2ace: a throw raised through a host function called from the lane (EventEmitter listener throws)
  escaped every `try/catch` in the main module: the unwind read the caller's handlers from the
  wrong program. Introduced by 184c2403; bisected and fixed with a regression test. The Node
  manifest goes from 582 to 584 passing with no new failures (813 listed; `test-http-server`
  fails before and after).

## Measurement notes

- Host: 4 vCPU cloud container, Linux 6.18.44; Node v26.12.0 `--jitless`; Bun with `BUN_JSC_useJIT=0`.
- Throughput: bombardier, 500 connections, 5 s per route, ping → query → body in one server process;
  peak RSS is the server's VmHWM after the three routes. Single runs vary ±10–15% on this host.
- Instruction counts: Callgrind over 2000 requests after 50 warm-up requests, A/B binaries in one session.
- Memory attribution: `/proc/<pid>/smaps` before and after load, `strace -k` on large mappings,
  massif on a system-allocator build, and the quench-alloc live-bytes counters (temporary build).
