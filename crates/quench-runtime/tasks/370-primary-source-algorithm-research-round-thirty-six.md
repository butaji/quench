# 370 — Primary-source algorithm research, round thirty-six

Status: complete

Find algorithms not already represented by the existing task ledger, constrained to OXC
plus Rust, rustc/LLVM-cooked templates, copy-and-patch linking, immutable shared kernels,
stencil-only first execution, no runtime hotness threshold, and no source/benchmark-shaped
selection. The authoritative exact V8v7 score entering this round is
`2390.00712577743`; research does not change that measurement.

## Local filter

The ledger already covers word-sized values, shapes, IC slabs, BBV/SSA, effect and ownership
analysis, escape/scalar replacement, arrays, strings, RegExp, call continuations, code
layout, register residence, symbolic PC, relocation closure, and total micro-op cover. New
papers therefore qualify only when they remove a measured seam, allocation, guard, patch,
or dependency that those mechanisms do not already name.

The source audit also rejects a speculative diagnostics task: prototype, region, call,
effect-reentry, opcode, and block counters sampled in `src/dynjit.rs` are guarded by named
runtime flags. The ECOOP 2026 evidence that broad type-feedback collection can cost about
1.2x on average and records much unused data supports the existing demand-derived policy,
but does not identify an unconditional tax here:
<https://doi.org/10.4230/LIPIcs.ECOOP.2026.16>.

## New mechanism: assumption-polymorphic native continuations

Task 371 imports Deoptless's central operation. A failed native assumption need not
materialize the whole canonical frame and enter one generic path. It can select the most
specific compatible native continuation for the exit's current context and transfer to it
directly. This is not a new tier and not a runtime compiler: finite candidates are quoted,
selected, composed, patched, and published as immutable instances at function-link time.

The canonical data is:

`ExitKey = ExitPc × ExitReason`

`ContinuationKey = ExitKey × ContextId`

`ContinuationCatalog = Map<ExitKey, NonEmpty<ContinuationInstance>>`

Selection is a pure partial-order query: filter candidates whose required context accepts
the observed machine state, then choose the unique most-specific candidate; ambiguity is a
link error, and the canonical generic stencil continuation is the total fallback. Only
site-local entry publication and the final machine transfer are effects.

Deoptless describes native-to-native OSR from failed speculation into specialized
continuations, selection by context predicates, and bounded reuse per exit:
<https://arxiv.org/abs/2203.02340>. Task 371 adapts the algorithm to prelinked
copy-and-patch alternatives and the project's no-interpreter rule.

## Refinements to existing mechanisms

1. **Prepatched specialization sets — Task 48.** The 2025 copy-and-patch JIT for R shows
   the value of pre-patching typed alternatives and selecting after observation. Here the
   alternative instances remain immutable and shareable; a site atomically switches a
   dispatch reference instead of overwriting executable bytes:
   <https://fikovnik.net/publications/vmil25.pdf>.
2. **Property-condition sum — Task 368.** JSC's Presence, Absence, and Equivalence
   conditions turn positive loads, negative lookup, and stable-value methods into one
   watchpoint algebra. This extends current positive prototype-chain plans without adding
   separate semantic paths: <https://webkit.org/blog/6756/es6-feature-complete/>.
3. **Canonical context reuse — Tasks 42/144.** Normalize live fact products before block
   version lookup and hash-cons equal requests. Context-guided splitting research reports
   that many separately created specializations are redundant; deterministic compile-time
   reuse fits the no-hotness rule: <https://kar.kent.ac.uk/109418/>.
4. **Counterfactual profiling — Task 331.** Causal profiling ranks a candidate by predicted
   whole-program response to speeding it up, which can reject attractive local costs that
   are not on the serial critical path: <https://arxiv.org/abs/1608.03676>.
5. **Already covered, do not duplicate.** The latest JSC allocation-sinking account maps
   to Tasks 176/191/369; CacheIR maps to Tasks 145/153; Maglev maps to Tasks 144/171; Swiss
   tables map to Tasks 91/200; mutable heap-number slots map to Task 204; Merkle-addressed
   AST/code storage maps to Tasks 42/80; and typed ownership/effect IR maps to Tasks
   171/172/173/177.

## Priority after this round

Do not let the new continuation mechanism jump ahead of the measured structural blocker.
The immediate sequence remains:

1. Task 367 audits whether the rustc/LLVM cooker preserves the intended tail-call and
   connector shapes.
2. Tasks 309/157/316/348 make the native micro-op cover total across effect edges.
3. Task 366 erases routine physical-PC/site materialization.
4. Tasks 144/158/171/203 retain typed values and facts across the now-continuous region.
5. Task 371 then prevents the remaining failed assumptions from collapsing back into a
   coarse generic frame transition.

Every retained experiment must state the exact dependency chain it removes, use named
limits, pass correctness/category-law tests, and survive Task 365's exact randomized paired
V8v7 gate. Published gains are motivation, never a projected score.

## Source chronology correction

Deegen is not a 2026 design. The paper was first submitted on 18 November 2024:
<https://arxiv.org/abs/2411.11469>. Keep that date in future citable descriptions.
