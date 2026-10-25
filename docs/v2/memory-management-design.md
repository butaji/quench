# Memory management strategy

Owner: memory-management lane (`v2-gc`, Linear TOD-137), by the user's
direction of 2026-10-17: the lane owns memory management as a whole, as a
concept and a strategy, for every workload Quench is measured on: V8-v7,
JetStream 3, Wasm CoreMark, the web framework benchmarks, and Node/npm
programs in general. Effect sizes marked *estimate* are projections, not
measurements (decision 20).

## 1. The model

For any run, the number the benchmarks report is the peak:

```
max RSS = floor + code + Σ_kind live(kind) × overhead(kind)
        + headroom (garbage not yet collected) + transients + retention
```

- **floor**: what a process holds before user code runs (binary pages,
  libraries, host bootstrap heap).
- **code**: bytecode, lowered Wasm, caches, source text kept for
  `toString`.
- **live × overhead**: reachable data times its representation cost
  (headers, cell size rounding, malloc rounding, arena slack).
- **headroom**: dead data waiting for the next collection; set by pacing.
- **transients**: short-lived peaks outside the managed heap (parser
  arenas, Wasm lowering buffers, JSON and regex compilation).
- **retention**: data that is unreachable in intent but reachable in fact
  (leaks: rooted finished jobs, caches without bounds, per-request state).

Time spent on memory is `allocation + collection (mark + sweep) + barrier +
page faults`. A design is good when each term is proportional to the work the
program asked for, never to the size of something the program did not touch.

**Principles.**

1. Every byte has one owner, one accounting point and one release rule. If
   memory cannot be named in the census, it does not exist in the design.
2. Costs are proportional to what changes: a collection pays for survivors
   and for what it frees, not for the old heap; a dead object costs a bitmap
   bit, not a `free()`.
3. Max RSS is a peak (decision 26). Only reductions at the peak count:
   floor, overhead, headroom, transients, retention. Returning pages after the
   peak is for steady-state servers, never a benchmark lever.
4. One memory controller decides when to collect, from one accounting of all
   bytes. No subsystem keeps a private budget.
5. Every mechanism is verified by an independent oracle (full trace,
   retainer walk, census) that runs in integration.

## 2. Where the memory is today, per workload

Linux x86_64, 4 vCPU, `v2-gc` after merging `v2-merged` on 2026-10-17,
one stock round per engine (a screen, not a result). Pacing in force:
`e14aadb13` (minimum 2^18 cells, 4x live, 4x full growth).

**V8-v7.**

| Bench | Score | Max RSS | Collections | Mark | Sweep | QuickJS RSS |
|---|---:|---:|---:|---:|---:|---:|
| Crypto | 2047 | 21.9 MB | 2 | 0.9 ms | 17 ms | 3.9 MB |
| DeltaBlue | 1174 | 20.2 MB | 3 | 1.2 ms | 19 ms | 3.9 MB |
| EarleyBoyer | 1782 | 47.0 MB | 70 | 68 ms | 598 ms | 17.5 MB |
| NavierStokes | 2437 | 5.8 MB | 0 | — | — | 5.5 MB |
| RayTrace | 2435 | 18.1 MB | 17 | 6 ms | 36 ms | 3.5 MB |
| RegExp | 743 | 26.9 MB | 59 | 57 ms | 603 ms | 6.8 MB |
| Richards | 1574 | 6.1 MB | 0 | — | — | 3.6 MB |
| Splay | 6035 | 92.4 MB | 7 | 135 ms | 134 ms | 156.6 MB (Bun 62.0) |

- Headroom sets RSS on Crypto, DeltaBlue, RayTrace and RegExp.
- Sweep, not mark, is the GC time where garbage is plentiful (about 10x mark
  on EarleyBoyer and RegExp, about 10% of wall each): every dead 96-byte
  `Cell` drops boxed payloads, `Rc` element vectors and string units through
  malloc.
- Every collection is full once any object record exists.

**JetStream 3** (from the JetStream lane's findings on Linear; the lane's
own all-test baseline in this session is in progress):

- Async and generator churn: 42% of a `for await` step and most of
  js-tokens is full collections over the Node bootstrap heap.
- Large retained sources: startup benches keep about 30 copies of 0.75 to
  2 MB source text plus bytecode; postcss marks its large live AST heap
  every few hundred KB because payload bytes advance the trigger while
  headroom is sized in cells.
- Byte-blind pacing: a loop dropping 200,000-element arrays peaked at
  163 MiB against Node's 96 MiB.
- Wasm tests: sqlite3-wasm 80.9 MB against Bun no-JIT 57.9 MB, from a
  ~40 MB lowering transient (all function bodies' operator vectors at once);
  argon2 spends about a quarter of samples allocating and sweeping
  `WasmV128` / `WasmBits64` heap cells.
- Retention bugs found so far: finished promise jobs rooted (763 to 211 MB
  over 12 iterations once fixed), environment chains keeping every loop
  iteration alive (891 to 163 MiB).

**Wasm CoreMark** (11 rounds, `v2-merged` `f71a0ee24`): Score 2,952 to 2,982,
ahead of Wasmi in 11 of 11 rounds; max RSS 6.99 MB static / 8.07 MB dynamic
against Wasm3's 4.16 MB. There is no GC in this run: the RSS is floor, code
pages (5.2 MB resident text when the link order misses executed functions),
linear memory and the lowered program.

**Web frameworks** (web lane, TOD-133): Hono 5-second screen at 66 MB
against Node `--jitless` 99 MB and Bun no-JIT 25 MB; after about 20,000
requests Quench grows to 190 to 230 MB (unexplained retention). The collector
is about 20% of a request's instructions (300 to 400 instructions per marked
cell) and glibc malloc/free about 10%. A generational prototype for records
was flat on Hono because per-collection fixed costs (rebuilding root tables,
owner-edge maps and shape constructor vectors every collection) consumed the
savings.

## 3. Taxonomy: every kind of memory, its owner and its strategy

### 3.1 Managed heap (JavaScript objects and everything they reach)

- **Non-moving, sticky-mark generational collection.** Old is the mark bit,
  kept across collections. Per-object states: `Free -> Young -> Old`,
  `Old <-> Remembered`, plus `Permanent` for the bootstrap image. A minor
  collection traces roots, remembered owners and young survivors, and sweeps
  only `occupied & !marked`. A major clears marks (except Permanent), traces
  everything and compacts metadata (property arena, shapes, programs).
  Chosen over a copying nursery because handles are integers held by Rust
  code and identity-keyed tables would need rehashing (V8's MinorMS lost to
  its Scavenger, but that baseline has JIT-inlined bump allocation Quench
  does not).
- **One write barrier at owner granularity**, in the two mutable gateways that
  already carry every write (`RecordSpace::get_live_mut`, `Heap::get_mut`):
  if the owner is marked, clear its mark and log it. Clearing the mark is
  the dedupe. Promise state and suspended awaits live in their own cells
  (`Cell::Promise`, `Cell::AwaitState`, which is both handlers of its await's
  reaction) and are traced with them; the remaining owner-attached VM edges
  (descriptors, promise job tables) call the same gateway when an edge is
  added. Object
  remembering, not card marking (cards rescan old objects on young stores).
- **Fixed per-collection cost must be near zero.** Roots and owner edges are
  maintained incrementally (dirty sets), not rebuilt by scanning every table,
  shape and descriptor at each collection. This is the web lane's lesson and
  a precondition for small young budgets.
- **Size-segregated families for every kind.** Objects and arrays are already
  fixed-size records in reserved ranges with side bitmaps. Strings,
  environments, functions, collections, iterators and generators move to the
  same machinery, generated by macro from one layout table that is the only
  authority for size class, edge visitor, byte accounting and finalizer. The
  96-byte `Cell` enum slabs and the legacy slot path are retired.
- **Payloads inside the managed heap.** Element backings and string units
  become accounted, size-classed families; small arrays keep elements inline
  in the record; copy-on-write sharing uses a shared flag, not `Rc` counts. A
  dead object is then cleared from a bitmap word. Kinds that own native
  resources (regex automata, buffers, Wasm memories, sockets) are on an
  explicit finalizer list swept separately.
- **Representation compaction** where it is measured to pay: one-byte
  strings, bounded-depth ropes, interned atoms; slack-tracked inline-slot
  classes per constructor; metadata as `u32` inside layouts. Not 32-bit
  `Value` (it would box doubles in an interpreter).
- **Allocation** into the lowest-address recyclable block of a family; a
  fresh block is a pure bump; untouched pages stay non-resident.
- **Precise roots.** Conservative stack scanning is unsafe with integer
  handles.

### 3.2 Allocation elimination (memory never allocated)

The cheapest memory is never allocated. Bytecode-level, guarded, with exact
fallback (AGENTS.md):

- for-of over built-in iterators without iterator-result objects;
- `arguments` not materialized; destructuring and spread temporaries reused;
- per-`await` reaction and promise objects reused where the spec allows;
  generator and async frames allocated once per activation;
- Wasm scalar values (i64, v128, tagged results) never become heap cells;
- allocation-site survival feedback: sites whose objects survive are
  allocated old (pretenuring without copying).

These serve JetStream (async, generators, tokenizers), web servers (request
objects, promise chains) and V8-v7 (EarleyBoyer, RayTrace temporaries).

### 3.3 Wasm memory

- **Linear memory** is a reservation with commit-on-touch (`mmap` with no
  prefault), grown in place, with guard regions where the address space
  allows. Its bytes are charged to the controller as external bytes so a
  program that drops instances triggers collection, but never traced.
- **Tables and globals** hold references through the same barrier gateway.
- **Compile transients** are bounded: lower one function body at a time
  (leaf bodies first for inlining), release operator buffers per body. The
  Wasm lane owns the decoder (decision 32); this lane owns the bound and the
  measurement.
- **Lowered code** is sized by a census per module (bytes per Wasm
  instruction), with cold functions lowered on first call.
- **CoreMark's RSS is floor and code**: the strategy for it is §3.6 and
  §3.4, not the collector.

### 3.4 Code and program memory

- Source text kept for `Function.prototype.toString` is stored once per
  program and shared by every function; large sources may be kept in a
  compact (one-byte) form.
- Bytecode is compiled on first call for functions that are never run at
  startup (JetStream's large libraries).
- Programs, their constants and caches are reclaimed when no function,
  environment or activation can run them (exists for dynamic programs; extend
  to every program kind).
- Inline caches and feedback are sized by execution, not by bytecode size.
- Parser arenas are released before execution starts.

### 3.5 Host and native memory (Node APIs, servers)

- Buffers, sockets, streams and timers are accounted to the controller at
  allocation and released by explicit lifecycle state machines.
- Per-request state is verified to be unreachable after the response: a
  retention check runs N requests, collects, and compares the census with
  the census after one request (tool in §5). The 190 to 230 MB Hono growth
  is a retention bug until shown otherwise.
- The remaining malloc traffic (outside the managed families) is measured by
  call site. An allocator A/B (glibc with the current tcache setting,
  mimalloc, a size-class pool for the hottest small sizes) is run once the
  families have removed most traffic, judged on Score and max RSS together.

### 3.6 The floor

Owned with the RSS lane (decision 9): lazy host modules and lazy global
properties, start-up-ordered code layout (trained on the general corpus,
decision 13), and in reserve a read-only bootstrap image (bytecode, shapes,
intrinsic templates) mapped and demand-paged. With generations, everything
alive after bootstrap is marked Permanent so no collection traces it.

## 4. Policy: one memory controller

One component owns the decision to collect and reads one accounting of all
bytes: managed families, payloads, external bytes (Wasm memory, buffers),
code. It is a named state machine (`Mutating -> MinorDue -> MajorDue ->
Collecting -> Mutating`, plus `Pressure` when a soft limit is near).

- **Young budget**: a fraction of live bytes with a floor of a few MiB (Lua
  5.4's 20% minor rule is the prior), tuned by measurement.
- **Major limit**: `M = L + sqrt(L · g / (c · s))` (MemBalancer; V8 ships the
  same rule as `sqrt_allocation_limits`), with `L` live bytes after the last
  major, `g` smoothed allocation rate, `s` smoothed collection speed, `c` one
  tuning constant; clamped to [1.1 L, 2 L], floor of a few MiB instead of
  2^18 cells.
- **Soft limit** for servers (Go's `GOMEMLIMIT` model): tighten pacing as
  the limit approaches, cap collector CPU share to avoid a death spiral.
- **Idle collection** for event-loop programs: when the loop is idle,
  collect and decommit empty blocks (steady-state RSS for servers; not a
  benchmark lever).

The constants (`c`, young fraction, floors) are the only tuning knobs; they
are owned here and boarded on all four suites before any change.

## 5. Tooling (memory is managed only as far as it is measured)

| Tool | Answers | State |
|---|---|---|
| Census by owner (`profile-memory`, heap census API) | What is live, by kind and payload | Exists; extend to every family, Wasm, code and host buffers |
| Peak decomposition (smaps at the peak + census) | What the peak is made of | To build: reconcile within 5% |
| Collection profile (`profile-gc-time`) | Mark, sweep, roots, per-collection fixed cost | Exists; add minor/major split, survivors, remembered |
| Verifiers: minor vs full trace; barrier completeness walk; major vs serial trace | Collector correctness | Exist under `debug_assertions` only; must run in integration |
| GC stress mode: collect at every safepoint | Rooting bugs (Rust-held Values across guest calls) | To build |
| Retainer path ("why is this alive") | Leaks and retention | To build |
| Request-retention check for servers | Per-request leaks | To build |
| Allocation-site profile | Survival per site; elimination targets | To build |
| Paired runner, all-eight instruction guard, minimal `posix_spawn` RSS helper | Decisions | Exist |
| Pinned RSS rounds (`rss-exec --cpu`, `v8-v7-rss.cjs --rss-cpu`) | Reproducible peaks: unpinned, a 2–4 MB process's peak reads up to a few hundred KiB low at random (§8.3) | Exist |

## 6. Staged path

Every stage is an experiment (decision 14) judged on all four suites where it
applies, Score and max RSS separately, no trade (decision 5).

| # | Stage | Serves | *Estimate* |
|---|---|---|---|
| 0 | Measurement: baselines on V8-v7, JetStream 3, CoreMark, web; peak decomposition; verifiers in integration; GC stress mode | All | — |
| 1 | Incremental roots and owner edges (near-zero fixed cost per collection) | Web, JetStream churn | Per-collection floor −50 to −90% |
| 2 | Sticky-mark generations over records and cells, barrier gateway, minor verifier | JetStream async/generators, web, EarleyBoyer, RayTrace, RegExp | Churn GC time −40 to −80%; barrier +0.5 to 2% |
| 3 | One controller: byte accounting everywhere, young budget, sqrt major limit, MiB floor | V8-v7 headroom benches, JetStream payload-heavy, web | Max RSS −30 to −70% on Crypto, DeltaBlue, RayTrace, RegExp |
| 4 | Payloads as accounted families; small arrays inline | EarleyBoyer, RegExp, JetStream parsers | Sweep −50 to −80% |
| 5 | Remaining kinds as families; legacy slot path deleted; Wasm scalars off the heap | All; argon2-wasm | Live heap −10 to −35% |
| 6 | Retention: request-retention check, retainer paths, fixes | Web, JetStream | Hono steady state toward its 5 s level |
| 7 | Allocation elimination and survival-feedback pretenuring | JetStream, web, Splay | Allocations −20 to −50% on async/generator code |
| 8 | Code memory: shared sources, compile on first call, program reclamation; bounded Wasm lowering transient | JetStream startup and Wasm tests, CoreMark | sqlite3-wasm −20 to −40 MB |
| 9 | Bootstrap generation (Permanent) and, in reserve, a read-only bootstrap image | All, floor | Major mark −50 to −90% on small-live programs |
| 10 | Allocator A/B for the remaining malloc traffic; slack-tracked inline slots | All | Measure |
| 11 | Server policy: soft limit, idle collection and decommit | Web | Steady-state RSS |

Research reserve, opened only on measured need: mature reference counting
(LXR) for old-generation churn in long-running servers; concurrent marking
for pause tails; parallel major marking on hosts with spare cores
(decision 3).

Not adopted: copying nursery, card marking with sticky bits, conservative
stack scanning, 32-bit `Value`, page return as a max-RSS lever.

## 7. Sources

Demers et al., POPL 1990 (sticky mark bits); WebKit Riptide; Blackburn and
McKinley, Immix (PLDI 2008) and MMTk StickyImmix; Zhao, Blackburn and
McKinley, LXR (PLDI 2022); Kirisame, Shenoy and Panchekha, MemBalancer
(OOPSLA 2022); V8 heap controller, Orinoco scavenger and concurrent marking,
slack tracking, pointer compression, snapshots; Hermes Hades; SpiderMonkey
nursery and pretenuring; QuickJS; Lua 5.4 generational mode; Go GC guide
and soft memory limit; Wingo on Nofl field-logging barriers, V8 MinorMS and
pretenuring. Full notes with links: the Linear design document.

## 8. State and evidence (2026-10-25)

Linux x86_64, 4 vCPU, static + BOLT `quench-node` unless noted. Each line names the Linear
issue whose comments hold its measurement and host (TOD-344: code in Git, evidence in
Linear). A component counts as settled when its alternatives were measured on the same
binary and lost.

### 8.1 What is in force

| Term of §1 | Mechanism | Evidence |
|---|---|---|
| headroom | `MemoryPolicy` selects one of two pacing tables; MemoryFirst's young minimum is 2^12: against 2^13, max RSS −76 to −416 KiB on 14 of 16 V8-v7 rows for Score ×0.86–0.91 on four fixtures (768 cost ×0.80 Score) | TOD-137 (young-minimum-4096), TOD-167 (young-and-release-sweep) |
| code (steady state) | MemoryFirst releases read-only pages and runs `malloc_trim` at every collection boundary | TOD-137 (collection-code-release) |
| transients (compile scratch) | MemoryFirst compiles loop regions without Cranelift's e-graph pass (`LOOP_TIER_OPT_LEVELS`): `--jit` max RSS Crypto −576 KiB, NavierStokes −648 KiB, for Score −2.8% / −6.1%; Throughput unchanged | TOD-137 (loop-tier-opt-level) |
| transients (compile) | Read-only pages released before every native compile (Wasm, JS function, OSR region); read-only and free malloc pages after every JS compile: `--jit` max RSS −1 to −3.5 MB on five fixtures | TOD-137 (jit-compile-release) (TOD-185) |
| retention (strings) | Regexp replace reads a rope through one unkept flattening: RegExp −0.5 to −0.6 MB | TOD-137 (replace-unkept) (TOD-184) |
| live × overhead | Record families with narrow inline slots, COW array elements, narrow arena vectors (TOD-147/148/162/166/168/170) | earlier evidence on those issues |
| start-up state (VM) | Direct-mapped memos (shape, atom, symbol proof, private home, function name, enumerable keys) allocate on first record, out of line: V8-v7 max RSS −48 to −254 KiB, Score within the A/A floor | TOD-137 (lazy-memo-tables) |
| start-up state (host) | Host operations found by a sorted name index | TOD-21 (operation-index) |
| allocator | The global allocator follows the policy: quench-alloc's size classes under Throughput, glibc under MemoryFirst; one reservation makes ownership an address compare, so frees route to the allocator that made the block | TOD-167 (policy-allocator) |
| transients (warm-up) | Under MemoryFirst, first-call compile-phase releases may run 5 ms apart (not 100): V8-v7 no-JIT max RSS −124 to −516 KiB on six fixtures; npm under MemoryFirst +7 to +21% time | TOD-137 (first-call-release) |
| start-up state (snapshot) | Startup intrinsic roots read from the sealed holders, not a copied list; `process.config` frozen natively | TOD-21 (startup-intrinsic-roots), TOD-21 (native-config-freeze) |
| retention (host weak roots) | A collected HTTP response releases its owner's weak root and async context whichever path retires it; keep-alive servers no longer keep one `WeakRef` cell per served response: hono-style 300K requests peak RSS 31–35 → 19.5 MB, wall −4% | TOD-137 (http-response-weak-root) |

### 8.2 Measured and rejected (same-binary A/B)

| Candidate | Result | Evidence |
|---|---|---|
| Release code only near the peak | Fires at almost every collection in steady state; Score ×0.87 | TOD-137 (collection-code-release) |
| Release code at power-of-two boundaries | Score +5%, max RSS +4%: a fixed trade, since a steady cycle reaches the same peak each time and needs a release each cycle | same, TOD-183 |
| Fault-around in the hot text only | Faults −30%, Score unchanged | same |
| Root and frame skipping on minor collections (TOD-164 step 3) | Within noise; collector instructions are 5–7% of the run | TOD-167 (young-and-release-sweep) |
| Weigh payload bytes more toward collections | RegExp and Crypto unchanged; Score −4 to −10% | TOD-181 (payload-unit-sweep) |
| Trim array slack at full collections | No max RSS gain; the tails stay in malloc's arena | TOD-137 (array-slack-trim) |
| Load and compile-phase release interval 0–20 ms under MemoryFirst | Max RSS unchanged (0.616–0.620 of Throughput) | session sweep, TOD-137 comment |
| `single_pass` register allocation for the JIT | −1.4 MB on NavierStokes `--jit`, Score −65% (Crypto −640 KiB, −22%); `opt_level=none` for every tier, measured −20% before, now costs −3 to −5%, and its loop-tier part is kept under MemoryFirst (§8.1) | TOD-110 comment, TOD-137 (loop-tier-opt-level) |
| Cranelift verifier off | No max RSS change on any `--jit` row | TOD-137 (loop-tier-opt-level) |
| Allocate the memos on first record with the allocation inlined | DeltaBlue `--jit` −8% over 20 alternating runs; making it cold restored the speed | TOD-137 (lazy-memo-tables) |
| First-call release at every first call / geometric back-off | Every call: npm 1.4–2.9× slower; back-off: V8-v7 `--jit` up to +312 KiB | TOD-137 (first-call-release) |
| Lazy `process` / EventEmitter for V8-v7 | No payoff possible: `console.log` writes through `process.stdout` (an EventEmitter) in Node and Quench, and every fixture prints | TOD-21 comment |
| MemoryFirst old growth 1/2–1/8; allocation cap 1/2–1/8 | Old growth: no consistent change. Cap 1/4: RegExp −260 KiB but Richards +128, Crypto `--jit` +140, EarleyBoyer Score −11% | TOD-137 (full-pacing-sweep) |
| Trailing first-call release from a helper thread | Removes Richards' warm-up spike, but a later peak is as high; RayTrace +300 KiB | TOD-137 (trailing-release-rejected) |
| Dense array growth 1.5× | Crypto −88/−100 KiB; DeltaBlue `--jit` +184, RayTrace `--jit` +128, RegExp +108 | TOD-137 (array-growth-rejected) |
| Charge property-arena growth toward the next collection | Max RSS 0.619 → 0.617 of Throughput (RegExp −0.2 MB), Score −1.6% | branch `meas-gc-charge-arena`, TOD-137 comment |
| Release the worker stack below the current frame at MemoryFirst collections | −148 to +216 KiB per row (5 runs, unbolted same-layout static builds): no consistent change; the stack is about 110 KiB resident and regrows to the same depth | branch `cand-gc-stack-release`, TOD-137 comment |
| `MADV_RANDOM` on the clean segments under MemoryFirst (no readahead into the page cache) | +56 to +228 KiB on 14 of 16 rows against current v2-gc (5 runs, unbolted static, same tree) | branch `cand-gc-random-text`, TOD-137 comment |
| Earlier first collection under MemoryFirst (first young 512–2048 against 4096), to release warm-up code sooner | No consistent change in median or worst max RSS on six fixtures (9 runs, one gc-tuning binary); a trailing release from a helper thread, re-run on current v2-gc and judged on worst runs (15 runs each), raised the worst max RSS on all six fixtures (Richards 2896 → 3716 KiB) | branches `cand-gc-first-young`, `cand-gc-trailing-release2`, TOD-137 comment |
| Box a regex's complete matcher (`Regex` 784 → about 170 bytes until lowered) | RegExp −96 / +124 KiB, every other row within ±80: no measurable benefit, as most of RegExp's patterns lower anyway (7 runs, unbolted static) | branch `gc-regex-box-complete`, TOD-137 comment |
| Young 2^10 with page releases paced to ≥ 2 or 10 ms apart under MemoryFirst | RegExp −224 KiB (−7% Score), but Richards, Crypto, RayTrace and DeltaBlue +50 to +260 KiB: a time-paced release misses the peaks | TOD-137 (paced-release-young) |
| Bound the collector by pacing alone (most eager pacing, page releases on and off) | With releases off, garbage at the peak is −96 KiB on Crypto, −136 on RayTrace and +84 on DeltaBlue, so the collector is at its floor there; RegExp holds −432 and Richards −256, at Score −45% and −5% | TOD-137 (eager-collector-bound) |
| Drop regexp matcher-cache entries no live RegExp shares, at full or every collection | RegExp +4 to +228 KiB, other rows flat: the cached matchers are in use, and dropping them recompiles. At RegExp's peak, compiled regexps are ~0.5 MB live (Massif: cache-held `Regex` 185 KiB, lowered programs 179, literal finders 60, match sets and classes 60) | session A/B (7 runs, unbolted static), TOD-137 comment |
| MemoryFirst first-call release by code growth (256 KiB since the last release) instead of every 5 ms | Unbolted: Crypto worst −72 to −204 KiB, but NavierStokes `--jit` +124 and RayTrace +100; without the time budget npm CPU 1.4–2.2×. Bolted `cdsort`, 15 runs: RayTrace `--jit` worst +228, other rows ±120 both ways | TOD-137 (growth-release-rejected) |
| Release the code pages at fewer MemoryFirst collections: only once file pages pass the hot set (measured over four collections after a release) by 64–512 KiB; or skip, or gate, the heap trim | Releases fall about 20×, Score +3% to +19% (RayTrace, EarleyBoyer, RegExp), but worst max RSS +136 to +464 KiB on the four rows at 0.90–1.00 of the target; the trim alone changes Score −5% to +6% | TOD-197 (memory-first-speed-cost) |
| Hot-set gate on the collection-boundary code release (release only once file pages pass the last measured hot set by 32–256 KiB, measured over 2 or 4 collections), on the static PGO + BOLT binary | Richards worst no-JIT 2188 → 2760–2972 KiB, `--jit` 2428 → 2796–3008, Score −4 to +7%: on a small program the gate never fires, so every touched code page stays | TOD-197 (gate-pgo-bolt) |
| First-call release interval 5 / 20 / 100 ms × resident-file budget 512K / 1M / off (one PGO + BOLT binary, 16 rows × 9 runs) | Budget off: Crypto, NavierStokes and RayTrace up to 15% over; every arm's Score geomean within ±1%; 1M at 21 runs: RayTrace tail −60 KiB, Crypto and Richards higher | TOD-197 (release-knob-grid) |
| Memory-first memos at a quarter of their entries | Richards −128 KiB, RayTrace and DeltaBlue worst unchanged, DeltaBlue `--jit` Score −6 to −9% (21 runs) | TOD-197 (memo-quarter), branch `gc-memo-fit` |
| Headerless payload space for string buffers | Exact census of RegExp: only 5,659 strings own a buffer; at most about 191 KiB of overhead to remove, under half the gap | TOD-137 (string-payload) |

### 8.3 Where the peak is now (MemoryFirst, no JIT)

At each peak, sampled from `/proc` with resident pages attributed by symbol size:
- **Code:** 1.3–1.8 MB, of which the interpreter's own working set is 0.8–0.9 MB (`vm` and
  `dispatch_fast`). Quench's code pages are fewer than QuickJS's 2.9 MB, which includes libc.
- **Heap, besides start-up state:** the property arena is a 516 KiB malloc mapping after it
  doubles on Richards (`mremap` 132 → 260 → 516 KiB; 288 KiB resident), set by the allocation
  burst before the first collection. Charging its growth to pacing did not move the peaks.
- **Heap:** 1.7–2.1 MB on the small fixtures. 1.58 MB of it is start-up state: about 1 MB for
  the runtime's builtins, 958 native function objects among them, and about 0.6 MB for the
  Node host (EventEmitter 164 KiB, `process` 168 KiB). QuickJS's whole heap is 0.55 MB.

Against the target, max RSS ≤ 0.8 × the lowest other runtime, on the board of 2026-10-24
(TOD-137 (board-memory-first-js-tiers)). Each cell is the worst of 5 runs, the median in
brackets:

| Fixture | No JIT | `--jit` |
|---|---|---|
| Richards | 0.97 (0.85) | 0.90 (0.90) |
| DeltaBlue | 0.89 (0.89) | 0.93 (0.89) |
| Crypto | 1.03 (0.97) | 1.02 (1.01) |
| RayTrace | 1.05 (0.98) | 1.01 (0.98) |
| RegExp | 1.04 (1.03) | 1.08 (1.05) |
| NavierStokes | 0.93 (0.93) | 0.93 (0.93) |
| EarleyBoyer | 0.45 | 0.46 |
| Splay | 0.64 | 0.64 |

**Measurement precision (2026-10-25).** Since Linux 6.2 a process's resident counters are
per-CPU, and the peak that `wait4` and `VmHWM` report is taken from their approximate sum.
A process that migrates between CPUs leaves deltas unsynchronized, so on these 2–4 MB rows
the reported peak lands up to a few hundred KiB low at random. RayTrace on one binary read
2436–2824 KiB over 20 unpinned runs, and 2796 in all 20 runs pinned to one CPU. In-process
reads of the precise counters at every release showed 2896–2908 KiB at RayTrace's peak in
every run. Worst-of-5 against best-of-5 turns that noise against the side judged by its
worst run. Pinned to one CPU, every engine, non-PGO static + BOLT at v2-gc `10230ec8dd`
(5 runs, worst over 0.8 × QuickJS's best): Richards 0.82 / 0.81, DeltaBlue 0.91 / 0.91,
Crypto 0.96 / 1.00, RayTrace 1.015 / 1.015, RegExp 0.98 / 0.98, NavierStokes 0.87 / 0.87,
EarleyBoyer 0.44, Splay 0.25 (TOD-197 (pinned-rss)). RayTrace's pinned peak is a plateau
at collection starts, about 1520 KiB of anonymous memory and 1270 KiB of file pages. The
file pages are the text the program runs in the roughly 2 ms between collections; the
anonymous part is about 860 KiB of malloc data in use, 150 KiB of free sub-page fragments,
and the record spaces and stacks.

### 8.3a Why the memory manager is at its floor on the rows that miss

A MemoryFirst peak is the sum of terms; the memory manager controls some of them. For each
controlled term, the evidence below bounds what any setting of it could still remove.

| Term | What the memory manager controls | Bound | Evidence |
|---|---|---|---|
| Garbage at the peak | collection pacing | The most eager pacing the knobs allow, with page releases off, removes at most 96 KiB on Crypto, 136 on RayTrace and none on DeltaBlue, at Score −24 to −79%. RegExp still holds about 430 KiB of garbage, and Richards about 260, that only that pacing removes (Score −45%, −5%). | TOD-137 (eager-collector-bound) |
| Pacing alternatives | young size, release timing | Young 2^10 with paced releases; earlier first collections; trailing releases. None lowers most rows (§8.2). | TOD-137 (paced-release-young) and the §8.2 rows |
| Retention | what a closure keeps | Closures keep only what their code can read, with the trace bounded at twice a whole trace. | TOD-137 (closure-retention), TOD-137 (environment-trace-cost) |
| Compile transients | which tiers compile under the policy, and code release around them | Under MemoryFirst no JavaScript tier compiles: every `--jit` row peaks at its no-JIT level. | TOD-137 (memory-first-js-tiers) |
| Cold first runs | where records live | A cold-compiled entry runs from its kept form; no row's worst run is its first. | TOD-137 (cold-entry-kept-form) |
| Code pages | release at boundaries | Releases at every collection and paced first-call releases. Every finer release rule measured raised a peak, or cost npm 1.4–2.9× in time (§8.2). | TOD-137 (first-call-release) |

**Against reference counting.** QuickJS frees each object when its count drops to zero, so it
holds no garbage between collections: its working set above its own empty-process floor is
what the workload needs with no collector headroom. Quench's working set above its floor is
smaller than that on all eight fixtures. In the board's median runs:

| Fixture | Quench above its floor | QuickJS above its floor | Ratio |
|---|--:|--:|--:|
| Splay | 29,484 KiB | 157,508 | 0.19 |
| EarleyBoyer | 4,336 | 15,172 | 0.29 |
| Richards | 316 | 712 | 0.44 |
| DeltaBlue | 756 | 1,208 | 0.63 |
| NavierStokes | 1,916 | 2,572 | 0.74 |
| Crypto | 856 | 1,068 | 0.80 |
| RegExp | 3,832 | 4,356 | 0.88 |
| RayTrace | 696 | 744 | 0.94 |

The empty-process floors, from 7 evicted runs of an empty script, are Quench 2016 KiB and
QuickJS 2744 KiB (median).

The rows that miss the target are those whose workload above the floor is small:
- RayTrace needs 744 KiB above QuickJS's floor, and Crypto 1068.
- With such a workload, 0.8 × QuickJS's total requires a floor far below QuickJS's own, even
  with no garbage at all.
- Quench's floor is already 27% lower. What remains lies in that floor's code pages and
  start-up state.

What is left on the three missing rows (1–8% over the target) lies outside these terms:
- **Crypto and RayTrace:** code pages, about 1.2–1.5 MB at the peak, which the code-layout
  lane owns (TOD-171); and start-up state, about 1.5 MB, which the start-up lane owns
  (TOD-21, TOD-104).
- **RegExp:** compiled regexps, held live by the matcher cache and by live RegExp objects;
  pruning the cache raised the peak (§8.2).
  - Their 237 patterns held 523 KiB. Match sets as two-word bitmaps, wide literal finders
    built on demand and a boxed opt-in byte backend bring that to 309 KiB.
  - Effect on V8-v7 RegExp: median −256 KiB without JIT and −128 with it, worst runs −200 and
    −212, scores within noise (TOD-137 (regexp-compact-patterns)).
  - RegExp also holds about 430 KiB of garbage that only the −45% pacing removes.

Run-to-run spread is 100–300 KiB on these rows, with the peaks set by time-paced releases.
It is of the same order as the remaining gaps.

### 8.4 What remains, by owner

- **Code layout** (RSS lane, TOD-171): bolting v2-gc `3d7dc0656a` with the old profile gave
  anonymous memory −120 KiB but file pages +350 KiB on Richards. The board needs a retrained
  link order and BOLT profile before it shows this lane's gains
  (TOD-137 (bolt-board-after-memo-and-first-call)).
- **Warm-up spike** (this lane, partly addressed): DeltaBlue, RayTrace and RegExp peaked in
  their first 20–100 ms. Under MemoryFirst, first-call releases 5 ms apart lower those peaks.
  Releasing at every first call would lower them further, but costs npm 1.4–2.9× in time,
  because no unprivileged process can tell hot code pages from cold ones.
- **Start-up anonymous memory:** about 1.47 MB at Richards' peak, against QuickJS's whole
  heap of 0.55 MB. The host bootstrap adds about 363 KiB of malloc, and every scored run
  needs `process` and EventEmitter (`console.log` writes through `process.stdout`). The
  runtime realm is about 300 KiB (c2's lazy native table and intrinsic groups, TOD-21).
  The shape table, LaneStack and compile-cache entries are already resident only as used.

- **Start-up heap** (RSS lane, TOD-21 and TOD-104): lazy host modules and lazy builtin function
  objects. The checkpoint census sizes each item; at about 1.6 MB it is larger than every
  remaining gap.
- **JIT compile scratch** (jit lane, TOD-110): OSR regions lower to about 64 IR instructions
  and 11 blocks per bytecode, mostly exit write-back blocks. regalloc2 scratch scales with
  blocks × values, and this sets NavierStokes `--jit`. Under MemoryFirst the e-graph pass is
  off for loop regions. What is left at the peak is about 2.8 MB of regalloc2 state for one
  region of 3,893 instructions.
- **Score** (Score and jit lanes): ×1.1–1.8 without JIT and ×1.8–63 with it, outside memory
  management. MemoryFirst costs ×0.90–0.93 against Throughput, and the code release is a
  known part of that.
