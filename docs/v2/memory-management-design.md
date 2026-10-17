# Memory management design: target architecture and staged path

Owner: GC lane (`v2-gc`), Linear TOD-137. Status: design, 2026-10-17. Inputs: a
sweep of every memory-related Linear issue and decision, a source inventory of
the heap, and the literature (Riptide, Immix/StickyImmix, LXR, MemBalancer, V8
Orinoco and heap controller, Hermes Hades, SpiderMonkey, QuickJS, Lua 5.4, Go).
Effect sizes marked *estimate* are projections, not measurements (decision 20).

## What the collector costs today (measured)

Linux x86_64, 4 vCPU, `v2-gc` after merging `v2-merged` on 2026-10-17, stock
harness, one round (screen). Pacing in force: `e14aadb13` (minimum 2^18 cells,
4x live, full growth 4x).

| Bench | Score | Max RSS | Collections | Mark | Sweep |
|---|---:|---:|---:|---:|---:|
| Crypto | 2047 | 21.9 MB | 2 | 0.9 ms | 17 ms |
| DeltaBlue | 1174 | 20.2 MB | 3 | 1.2 ms | 19 ms |
| EarleyBoyer | 1782 | 47.0 MB | 70 | 68 ms | 598 ms |
| NavierStokes | 2437 | 5.8 MB | 0 | — | — |
| RayTrace | 2435 | 18.1 MB | 17 | 6 ms | 36 ms |
| RegExp | 743 | 26.9 MB | 59 | 57 ms | 603 ms |
| Richards | 1574 | 6.1 MB | 0 | — | — |
| Splay | 6035 | 92.4 MB | 7 | 135 ms | 134 ms |

QuickJS on the same host: 3.5 to 6.8 MB on six of these, 17.5 MB EarleyBoyer.

Three facts follow:

1. **Headroom, not live data, sets max RSS** on Crypto, DeltaBlue, RayTrace and
   RegExp: 2 to 17 collections in the whole run, 15 to 20 MB above the 6 MB
   floor. The 2^18-cell minimum alone permits roughly 16 to 24 MB of garbage.
2. **Sweep, not mark, is the GC time** wherever garbage is plentiful: about 10x
   mark on EarleyBoyer and RegExp (about 10% of wall each). Sweep cost is per
   dead object: dropping a 96-byte enum cell frees its boxed payloads, `Rc`
   element vectors and string units through malloc. Marking a small live heap
   is cheap even with the bootstrap heap in it.
3. **Every collection is full** once any record exists
   (`Heap::collection_is_full`), and pacing ignores most payload growth
   (ArrayBuffer bytes, arena, Map/Set entries, environments, rope flattening,
   copy-on-write detaches, programs). JetStream churn (async iteration,
   generator tokenizers) pays 42% of its instructions in these full
   collections.

The 10-16 pacing bought Score (Splay +20%, RayTrace +40% stock) by spending RSS.
The design below aims to keep that Score with the old (384-cell, 1x) RSS or
better, by making each collection cost proportional to what it frees and what
survives, not to the heap.

## Target architecture

**Non-moving, sticky-mark generational, size-segregated families, byte-paced.**

1. **One heap model.** Every collected kind lives in fixed-size families like
   today's object and array records: a reserved range, blocks with occupancy,
   mark and remembered side bitmaps. One macro-generated layout table is the
   only authority for each family's size class, edge visitor, payload
   accounting and finalizer. The 96-byte `Cell` enum slabs and the legacy
   slot minor path are retired family by family (strings, environments,
   functions, collections, iterators/generators).
2. **Generations as an explicit state machine per object:**
   `Free -> Young -> Old`, `Old <-> Remembered`, plus `Permanent` for the
   bootstrap image. Old is the sticky mark bit; nothing new is stored per
   object.
   - Minor: roots + remembered owners + owner-attached VM edges of remembered
     owners; marks young survivors (they become old); sweeps only
     `occupied & !marked` words; clears remembered bits.
   - Major: clears marks except Permanent, full trace, sweep, arena and shape
     compaction, program reclamation.
   - Weak: minors clear weak refs to young targets and prune entries in young
     and remembered weak containers; old targets survive a minor by
     definition. Ephemerons reach their fixed point over the same worklist.
3. **One write-barrier gateway** at owner granularity (object-remembering, as
   in Riptide and StickyImmix; not card marking, which rescans old objects on
   young stores). Today every record mutation already funnels through
   `RecordSpace::get_live_mut` and every legacy one through `Heap::get_mut`;
   the barrier lives there: *if the owner is marked, clear its mark and log
   it.* Clearing the mark is the dedupe; the minor re-marks and re-traces it.
   VM-side tables that attach edges to an owner (descriptors, promise
   reactions, async continuations) call the same gateway on the owner when
   they add an edge, so the per-collection scan of those tables goes away.
4. **Dead objects cost bits, not frees.** Payloads move into accounted,
   GC-managed storage: element backings and string units in size-classed
   families (small arrays inline in the record), boxed cold payloads in their
   own families. A dead object is then cleared from a bitmap word instead of
   walking `Drop` through malloc. Kinds that still own native resources
   (regex automata, buffers, Wasm memories) register an explicit finalizer
   list, swept separately.
5. **Byte pacing with two budgets** (TOD-125, MemBalancer, Lua 5.4):
   - Every allocation and payload growth is charged in bytes at one point
     per family (derived from the layout table), including rope flattening,
     ArrayBuffer and Wasm memory growth, and program installation.
   - Young budget: a fraction of live bytes with a floor of a few MiB.
   - Major limit `M = L + sqrt(L * g / (c * s))` with smoothed allocation
     rate `g` and collection speed `s`, clamped to [1.1 L, 2 L], with a
     4 to 8 MiB floor instead of 2^18 cells.
6. **Allocation** into the lowest-address recyclable block of a family, so
   fresh pages stay untouched; a fresh block is a pure bump. Lazy sweep per
   block only for families with finalizers.
7. **Precise roots** (handles are small integers, so conservative scanning
   would retain falsely). Parallel marking only for majors above a live-heap
   threshold (decision 3); no concurrent marking until a pause requirement
   exists.
8. **Allocation elimination** before the heap: for-of iterator results over
   built-in iterators, unmaterialized `arguments`, destructuring temporaries,
   per-`await` reaction reuse, all behind exact-fallback guards; then
   allocation-site survival feedback that allocates long-lived sites as old.

Not adopted, with reasons: a copying nursery (identity-keyed tables and
Rust-held integer handles); card marking (rescans old objects on young
stores); conservative stack scanning; 32-bit `Value` (boxes doubles in a
no-JIT engine); `madvise`/trim as an RSS lever (decision 26: max RSS is a
peak).

## Staged path

Each stage is an experiment with a decision rule (decision 14): 11 paired
rounds on all eight, max RSS and Score judged separately, no trade
(decision 5), the all-eight instruction guard, the runtime suite, and a debug
run of the full-trace verifiers.

| Stage | Change | Removes | Decide by | *Estimate* |
|---|---|---|---|---|
| 1 | Sticky-mark minor over records and legacy cells (G-09b): barrier in the two mutable gateways, owner-attached edges logged, minor sweep of `occupied & !marked`, verifier compares every minor with a full trace with old pinned | Per-collection cost scaling with the old and bootstrap heap | Correctness first; then stage 2 decides the policy | Churn workloads −40 to −80% GC time; barrier +0.5 to 2% mutator |
| 2 | Byte charging at one point per family; young budget in bytes; MemBalancer major limit with a small floor (G-15, G-16) | Garbage headroom at the peak | All-eight RSS lower than both the 10-16 and the 384-cell policy, Score no lower than 10-16 | Max RSS −30 to −70% on Crypto, DeltaBlue, RayTrace, RegExp; Splay toward 2x live |
| 3 | Element backings and string units as GC-managed, accounted families; small arrays inline | Sweep cost per dead object; malloc overhead | Sweep ns per freed object; Score; RSS | GC sweep time −50 to −80% on EarleyBoyer and RegExp |
| 4 | Remaining `Cell` kinds as families from the layout table; legacy slot path deleted | 96-byte cell waste; two collector paths | Live bytes per bench; Score | Live heap −10 to −35% |
| 5 | Bootstrap generation: one full collection after host bootstrap marks survivors Permanent; majors skip them except through the remembered set | Major mark of the bootstrap heap | Major mark time; Node startup | Major mark −50 to −90% on small-live benches |
| 6 | Allocation elimination and survival-feedback pretenuring | Allocation rate | Allocations per run; Score | Async/generator allocation −20 to −50% |
| 7 | Slack-tracked inline-slot families per constructor; fewer arena indirections | Per-object overhead | Score; RSS | Property-heavy benches +3 to +10% |
| 8 | Lazy sweep for finalizer families; parallel major marking on hosts with spare cores; idle-time collection for Node | Pause and steady-state RSS (not max RSS) | Pause tail; wall | — |

Research reserve, opened only on measured need: mature reference counting
(LXR) for old-generation churn in Node services; concurrent SATB marking for
pause tails.

## Verification that ships with every stage

- Debug builds verify every minor against a full trace with old objects
  pinned, and every major against an independent serial trace.
- A debug heap walk after each minor asserts there is no old-to-young edge
  whose owner is not remembered (barrier completeness).
- The verifiers run in integration through a debug-assertions test profile,
  not only `cargo test` with the iteration profile (which has none).
- Weak, FinalizationRegistry and ephemeron behavior is checked across minor
  and major collections, including old WeakMaps with young values.

## Ownership

The user directed on 2026-10-17 that the GC lane owns the collector and
memory-management design (decision 33 as amended). The RSS lane keeps the
process floor (decision 9).
