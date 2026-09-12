# 276 — Hot/cold stencil segments and shared obligation tails

Status: planned

Represent a linked function as two code monoids with the same connector category:

`HotImage = prologue ; fast bodies ; branches`

`ColdImage = guard misses ; coercions ; exceptions ; materializers ; helper adapters`

A `StencilTemplate` may contribute atoms to either segment and symbolic holes may target
labels in either. Final linking lays out the hot monoid contiguously, then the cold
monoid, resolves every cross-segment branch, and inserts a near-code island only when
the architecture range requires it. Closed cold composites with identical bytes,
external patches, and connector contracts freeze into one immutable shared `Kernel`;
function-specific materialization values remain copy-patched `StencilInstance`s.

This is a physical interpretation of the same quoted graph, not a second optimizer or
an eager mutation API. Associativity and identities hold independently within both
segments; `partition` is a pure homomorphism from the annotated stencil expression to
`Hot × Cold`. Static semantic annotations from Task 209 decide the segment. Runtime
counters, source names, benchmark names, and hotness gates are forbidden.

Use named constants for segment alignment, direct-branch range, maximum island bytes,
maximum copied cold bytes, and the kernel-sharing threshold. Never copy an unresolved
object-file dependency; Task 274's code/data atom graph remains the closure boundary.

Acceptance: disassembly proves fast fallthrough is contiguous and guard/error arms are
out of line; equivalent cold tails share one immutable address; all symbolic/internal/
external relocations remain closed; 94 release tests pass; linked hot bytes and i-cache
samples decline; complete V8v7 A/B must improve without a component-floor violation.

Primary sources: CPython's measured hot/cold Copy-and-Patch plan
<https://github.com/python/cpython/issues/115802>; LLVM machine block placement and cold
section concepts <https://llvm.org/docs/BlockFrequencyTerminology.html>.
