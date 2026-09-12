# 32 — Element-kind-guarded array connector

Status: complete

Evidence from the V8v7 suite: crypto's `am1`-`am4`/`bnpMultiplyTo` inner loops index parallel packed-numeric arrays; navier-stokes indexes flat numeric fields (`dens`, `u`, `v`) in tight nested loops; raytrace indexes `scene.shapes`/`scene.lights`; richards touches array-backed queues. Five of the eight benchmarks are array-bounds/element-kind dominated — broader payoff than pure arithmetic guards ([[10-specialized-arithmetic]]) alone.

Add an `ElementKindGuarded<K>` connector (packed-numeric vs. holey/tagged backing) per [[25-generalized-speculative-guards]], feeding a direct bounds-checked load/store stencil for the packed-numeric case and falling back to the generic property/array path otherwise. Depends on [[12-array-fast-path]] for the underlying dense-array representation and [[09-object-memory-model]] for a stable backing-store distinction to guard against.

Acceptance: crypto's and navier-stokes's innermost array loops compile to guarded direct indexing with a single element-kind check hoisted where provably loop-invariant (via [[21-loop-invariant-hoist-associativity]]); an array observed to transition from packed to holey correctly invalidates the guarded path; out-of-bounds access still produces correct semantic behavior through the generic fallback.

## Rejected experiment: generic numeric/dense runner

Runtime-frequency evidence in `reports/semantic-block-profile.jsonl` identified a
61-op numeric/local/dense-array block entered 1,566,022 times in Crypto and a 39-op
block entered 1,310,720 times in Navier-Stokes. A general `NumericDenseRun` stencil was
therefore tried for long, fully supported basic blocks. Selection used only bytecode
capability, dense access presence, and a named minimum length—never source identity.

The first smoke run exposed an `Illegal instruction`: LLVM had emitted a local switch
jump-table relocation and an external `_fmod` relocation inside copied bytes, while the
catalog represented only continuation holes. [[102-reject-unrepresented-aot-relocations]]
now prevents this class of malformed stencil. After removing those relocations, all 42
tests and the complete smoke suite passed.

The runner was still rejected and removed. The isolated six-repetition comparison in
`reports/numeric-dense-run-isolated-ab/comparison.txt` was too noisy and failed
component floors despite +2.04% aggregate. A longer three-repetition comparison in
`reports/numeric-dense-run-isolated-ab-long/comparison.txt` was effectively neutral:
744.112 → 744.972 (+0.12%); Crypto improved 19.21%, but Navier-Stokes regressed 2.37%
and Earley-Boyer 0.91%. The copied runner was another opcode-dispatch loop plus indirect
dense helpers, not C-like specialized machine code.

Next step: represent element-kind, bounds, backing pointer, and loop-carried numeric
state as explicit guard/connector data, then compose a straight-line or traced-loop AOT
stencil whose copied body contains no opcode switch and no per-element helper call.
Hoisting and invalidation remain required before this task is complete.

The implementation is now decomposed into four tracked, coarse layers rather than a
new opcode-runner experiment: [[120-typed-numeric-dense-region-ir]] quotes and proves
the region; [[121-hoisted-numeric-dense-guard]] establishes one reusable context;
[[122-rustc-aot-numeric-region-templates]] cooks unchecked body vocabulary with
rustc/LLVM; and [[123-traced-numeric-dense-loop-stencil]] closes and links the loop as
one executable function. These are abstraction-level monoids, not thousands of
independently guarded opcode fragments.

## Result: accepted through the traced region

Tasks [[120-typed-numeric-dense-region-ir]] through
[[123-traced-numeric-dense-loop-stencil]] complete the guarded-array connector. Packed
kind and backing facts are checked once at external entry; direct AOT load/store leaves
use the retained raw view and keep only exact index/bounds checks. Holey inputs and
resizing writes use canonical fallback. Crypto and Navier–Stokes execute 1.48M and
1.75M counted native backedges in the diagnostic run, and their exact A/B medians
improve 20.67% and 346.44% respectively. See
`reports/task123-traced-region-ab-6/comparison.txt`.
