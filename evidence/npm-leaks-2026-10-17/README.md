# Two leaks and a quadratic compile pass behind the npm RSS gap (2026-10-17)

Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM, no cgroup limits).
Comparators: Node v26.12.0 and Bun 1.4.2; Bun ran with this environment's
`BUN_OPTIONS=--smol` (see `../npm-gc-merge-2026-10-17/default-bun.txt`).

## Findings and fixes

1. **Every require/import leaked its resolver cache (1a60fec28).**
   - oxc_resolver 2.1.1 linked cached paths strongly in both directions: a path's
     parent, its `node_modules` child, its canonical form and its package.json scope.
     A dropped resolver therefore never freed its cache.
   - quench-node builds a resolver for each resolution, so each one leaked its
     cached paths and parsed package.json files.
   - `resolve-loop.cjs`: 7,000 `require.resolve` calls take RSS from 7.5 to
     592 MiB on Quench; Node stays at 71.
   - Fix: oxc_resolver 11, which holds those links weakly. The loop now ends at
     14.5 MiB.
2. **Every host-function call leaked a root list (3f8310a9e).**
   - `NativeContext::with_new_target`, which every host call uses, started with a
     new list, while `Drop` returned each list to `vm.native_root_lists`. The pool
     grew by one list per call and was never drawn from.
   - `host-call-loop.cjs`, 200,000 calls: `process.uptime()` 23 → 38 MiB and
     `fs.statSync` 93 → 311 MiB, with a 2 MiB heap.
   - Fix: both constructors draw from the pool. Every loop now stays at 18–21 MiB.
   - Regression test: `native_calls_reuse_root_lists_instead_of_accumulating_them`.
3. **TDZ elision was quadratic in memory (bab37aaa2).**
   - `Function::unobservable_tdz_slots` kept a bitset of every local for every
     instruction: 61 MiB while compiling lucide-react's 1.2 MB CommonJS bundle.
   - Fix: it now keeps one state per basic block, with the same fixed point.
   - A temporary cross-check asserted identical results for every function compiled
     by the runtime suite (700 tests), quench-node-test's child_entry suite and 24
     npm scenarios.

`api-loop.cjs` and `async-api-loop.mjs` probe the other families and stay flat:
events, crypto, zlib, Buffer, RegExp, Map/Set, TextEncoder, util.inspect, Error.stack,
promises, structuredClone, URLSearchParams, fs.promises, timers, dynamic import, http
round trips, read streams and spawnSync.

## Effect on the npm top 300 (release builds; all 300 pass in every run)

| run | change | lowest of 300 | median Quench / Node / Bun MiB |
| --- | --- | --- | --- |
| m4 (41baa502c) | before | 264 | 23.3 / 72.0 / 36.4 |
| m5 | + resolver | 267 | 22.9 / 72.1 / 36.5 |
| m6 | + root lists | 268 | 22.6 / 72.0 / 36.6 |
| m7 (bab37aaa2, sha256 `11c36147366357eb…`) | + TDZ | 269 | 22.6 / 63.5 / 36.8 |

Peak RSS of the largest packages (MiB):

| package | m4 | m7 | Bun |
| --- | --- | --- | --- |
| typescript-eslint | 331 | 270 | 145 |
| playwright | 230 | 174 | 103 |
| @testing-library/dom | 223 | 143 | 119 |
| jsdom | 201 | 121 | 99 |
| lucide-react | 101 | 44.5 | 57 |

`npm-scenarios.json.gz` and `rss-report.txt` are m7.

Of the 31 packages where Quench is not lowest:
- Eight are decided by TypeScript 7's native `tsc`, which every engine execs.
- Five are within 2%.
- The rest are code-heavy: compiled-program memory and the parse arena of large
  bundles (`../npm-gc-merge-2026-10-17`, TOD-123).

## Instruction guard (`callgrind-guard.txt`)

`npm-icu-2026-10-16/npm-icu-fixed-callgrind-guard.sh` with iteration builds:
- Base: v2-merged c7b5dca1b.
- Candidate: that base with these fixes merged in.

Per-run instructions change by −0.00% to +0.05% on all eight fixtures. Startup is
about 0.9 M instructions lower on each.
