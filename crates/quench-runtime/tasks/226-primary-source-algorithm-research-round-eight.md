# 226 — Primary-source algorithm research, round eight

Status: complete

Research further compiler/VM algorithms that fit the standing architecture: every
function executes a stencil graph from first use; rustc/LLVM cooks a finite catalog AOT;
runtime generation is copy, patch, and semantic-case IC repair only; no interpreter,
hotness threshold, benchmark identity, or exact source sequence selects execution.

## Findings

The most useful newly actionable algorithm is **Static Basic Block Versioning**, not
another leaf stencil. The existing Task 144 used runtime first-arrival contexts. SBBV
instead breadth-first traverses the quoted CFG before emission, propagates branch facts,
creates typed block versions, and delays context merging until a per-block version budget
is exceeded. Its published evaluation reports roughly ten-percent average execution-time
improvement and 54–62% fewer dynamic type tests with a two-version limit in its two AOT
Scheme compilers. Those numbers are evidence for trying the algorithm, not a prediction
for this JavaScript VM.

Task 144 now owns the one canonical implementation: a work queue of `(block, Gamma)`
demands, a representation/fact lattice, conservative joins, most-similar context merging,
edge redirection, and derived reachability. `MAX_STATIC_BLOCK_VERSIONS` names the budget.
This is a particularly good fit for the Lisp staging discipline: quote CFG data, expand
typed versions to a fixed point, normalize, then emit bytes once.

Five existing tasks absorb the other findings rather than duplicating knowledge:

1. **IC impossible sentinels and proof hoisting (Task 145).** Deegen can initialize an IC
   key to a value excluded by the key domain, erasing the separate occupancy branch. A
   call-IC hit already proves callability, so the callability test belongs only on the
   miss arm. Most importantly, an IC effect arm operates directly on the caller's machine
   state; treating every arm as a C-ABI kernel recreates the seam the slab should remove.
2. **Pointer-bump guest calls (Task 146).** Deegen uses a custom guest stack and pinned
   continuation state. The current `CallFrameLayout`, pooled `Vec<Value>`, `DynFrame`,
   cloned environment, and recursive Rust entry are therefore not incidental overhead;
   they are the remaining non-native call abstraction and must disappear together.
3. **Relative array bounds plus loop widening (Tasks 214/196).** SBBV's bound domain can
   retain `length(array) - constant` symbolically, narrow it on branches, and isolate a
   checked first iteration from an unchecked steady-state loop. This is a more direct
   implementation recipe for Task 175 than generic interval analysis alone.
4. **Logical-version bump allocation (Task 162).** JSC's block allocator refreshes mark
   and new-state bitmaps lazily using epochs and allocates from contiguous bump ranges.
   This is the smallest non-moving tracing-heap design compatible with stable object
   handles; full concurrent GC and opportunistic Immix evacuation remain unnecessary for
   the MVP.
5. **Mutable numeric binding cells (Task 204).** V8's binding-slot state machine owns an
   in-place mutable `Int32`/double cell and widens to tagged on incompatible writes. It
   removes repeated numeric allocation and lets compiled code keep integer operations;
   the published case improved one JetStream2 workload by about 2.5x and the overall
   suite by about 1.6%. Task 204 already described this representation and remains the
   correct owner.

## Region-size warning

HHVM's tracelet experience is directly relevant to the current residual profile: small
independently compiled regions repeatedly shuffle state, repeat type guards, and prevent
GVN, load elimination, reference-count elimination, and LICM. Its larger-region redesign
reported double-digit fleet CPU improvement, but depended on runtime PGO. Take the
**region-size lesson**, not its heat policy: SBBV and the quoted effect/ownership graph
form the largest statically valid regions, and every branch remains semantically complete.
This reinforces Tasks 128, 144, 171, and 172 and argues against more exact five-op
superinstructions.

## Priority

The research does not displace the measured top bottleneck. The implementation order is:

`146 + 203 -> 145 call slabs -> 144 static BBV -> 171/172 -> 148/162 -> 204 -> 214/196/175`

Tasks 146 and 145 must remove the host call/helper seam before more call-shaped stencil
wrappers are attempted. Static BBV then makes proofs and register contexts survive across
whole blocks and loops. The heap removes `Rc<RefCell<_>>` traffic; range and loop work only
pays fully after values can remain in raw machine representations.

## Primary sources

- Static BBV algorithms and evaluation:
  <https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>
- Lazy/interprocedural BBV background:
  <https://doi.org/10.4230/LIPIcs.ECOOP.2015.101> and
  <https://arxiv.org/abs/1511.02956>
- Deegen semantic derivatives, custom stack/register pinning, impossible IC keys, call-IC
  proof hoisting, inline slabs, and direct machine-state IC arms:
  <https://arxiv.org/html/2411.11469v2>
- HHVM tracelet-boundary and larger-region evidence:
  <https://hhvm.com/blog/2017/02/17/region-jit.html>
- JSC fixed-size allocation blocks, bump ranges, logical bitmap versions, and generational
  design: <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>
- V8 mutable numeric binding cells:
  <https://v8.dev/blog/mutable-heap-number>
- Immix mark-region alternative, retained only as later fragmentation evidence rather
  than MVP scope: <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>
