# 287 — Primary-source algorithm research, round nineteen

Status: complete

This pass searched recent primary papers and production-engine documentation for
general algorithms compatible with the standing architecture: stencil execution from
the first invocation, finite rustc/LLVM-cooked templates, immutable shared kernels,
copy/patch/share instantiation, no third-party VM, and no source-name, benchmark-identity,
execution-count, or hotness-based selection. In Lisp terms, every retained idea extends
the quoted recipe/fact graph or its one final link/evaluation; none creates an unrelated
imperative optimization path.

## New or materially sharper experiments

1. **PC-relative near-code closure (Tasks 189 and 274).** Put copied code, its local
   constants/tables, and shared-kernel islands in one range-checked arena. The R
   copy-and-patch implementation reports more than 8% average runtime improvement on
   compilable workloads and 26% smaller executable output from its PC-relative memory
   model. This is the strongest small, isolated experiment found and complements—rather
   than duplicates—the existing arena and relocation-closure tasks.

2. **Overlap-aware offline superinstruction discovery (Tasks 157 and 272).** Mine a
   bounded dictionary of normalized semantic/effect/context sequences and score a
   candidate only after accounting for overlap with other candidates. Candidate mining
   is offline and benchmark-independent; the generated typed matcher remains the only
   per-function selector. This fixes a blind spot in the current catalog plan: frequency
   alone overbudgets redundant overlapping bricks.

3. **Context-partitioned IC evidence (Task 163).** Key the mutable `InlineSite` plane by
   the same bounded structural context used for caller-customized immutable images.
   Order versions from specific to general and lattice-merge on budget overflow. This
   avoids one caller polluting another without introducing execution-count tiering.

4. **Microarchitectural falsification in the optimization loop (Task 197).** Capture
   disassembly and available CPU counters for dependency chains, branches, misses,
   instructions, and code footprint, but retain alternating end-to-end A/B as the only
   acceptance gate. A recent negative IC study demonstrates why eliminating loads or
   patch operations is not sufficient evidence of speed.

5. **Allocator details remain refinements, not new tasks (Tasks 148, 158, 162, 193).**
   Immix/JSC-style regions and active bump blocks already belong to the stable-handle
   heap; Maglev's next-use allocation, rematerialization, edge moves, and separated GC
   spill metadata already belong to the region planner. Nofl's fine-grained precise
   reclamation is a later space-efficiency option after the object-only heap is correct.

6. **Validated reusable compiler-interface caching remains late (Task 80).** Replaying a
   preoptimized IR with explicit assumption validation and object relocation can reduce
   compile/warmup cost, but it does not attack the current steady-state V8v7 deficit.
   Keep it as a later refinement of the persisted-image cache, behind execution-path
   work.

## Ideas deliberately not promoted

- **Patch IC offsets into instructions solely to remove loads:** rejected as an
  optimization premise. The 2025 study measured no end-to-end benefit despite fewer
  loads; Task 197 now demands machine and A/B evidence.
- **Two-level virtual-memory guard pages for ordinary JS arrays:** useful for a linear
  Wasm-style sandbox, but this VM's independently allocated, shape-bearing JS arrays do
  not provide that address-space contract. Existing range/shape proofs are the correct
  abstraction.
- **Another interpreter-derived tier:** Weval and Druid validate generation from one
  semantic source, but the project already has stencil-only lowering and explicitly
  rejects an interpreter fallback. Their algorithms add no new execution tier here.
- **A second GC design or another hand-written register allocator:** these would duplicate
  Tasks 148/162 and 158/193 instead of strengthening the canonical representations.

## Revised implementation order

Finish the already-started object-only Task 148 cutover and restore all tests. Then run
the small Task 189/274 PC-relative arena experiment. Continue with Task 156/191 allocation
and initialization, Task 146/163 direct calls and context-partitioned feedback, Tasks
171/144/152/158 for cross-operation context/shape/register state, and Tasks 157/272 for
offline-mined typed supernodes. Apply Task 197's evidence layer to every experiment.

No score claim changed during this research pass.

## Primary sources

- Deegen: <https://arxiv.org/abs/2411.11469>.
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>.
- Copy-and-Patch JIT for R: <https://d3s.mff.cuni.cz/publications/kocourek_copyandpatch_2025/>.
- Negative inline-cache patching result: <https://arxiv.org/pdf/2502.20547>.
- Automated superinstruction synthesis with overlap handling:
  <https://www.xiaowenhu.com/files/splashws24vmilmain-p91-p-a53cc9a2c0-81338-final.pdf>.
- Reducing feedback pollution with context-partitioned feedback vectors:
  <https://skrynski.github.io/reducingFeedbackPollution.pdf>.
- V8 Maglev abstract state and register allocation: <https://v8.dev/blog/maglev>.
- JavaScriptCore shape speculation and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- SpiderMonkey CacheIR recipe model:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>.
- Immix: <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>.
- JavaScriptCore GC block allocator:
  <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>.
- Nofl precise Immix-style collector: <https://arxiv.org/abs/2503.16971>.
- Reusable optimized compiler IR with validation and relocation:
  <https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2025.25>.
- Performant Wasm guard-page bounds checking (reviewed but not adopted):
  <https://lukas-doellerer.de/files/vmil2024-preprint.pdf>.
