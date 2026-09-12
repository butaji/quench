# 238 — Debug-only runtime verification harness for erased guards

Status: planned

Practical safety net for the whole exactness/erasure family
([[231]], [[232]], [[233]], [[234]], [[235]], [[236]]), complementing
[[237]]'s adversarial static test catalog with a runtime check that costs literally
nothing in the release binary this project measures against the V8v7 gate. Rust's
`debug_assert!` is the standard instance of this pattern (compiled out entirely in
release builds, present and checked in debug builds); this task applies the same idea
specifically to erased guards, since they are exactly the place a silently-wrong proof
would otherwise be undetectable until it produces a visibly wrong program result far
from the actual defect's source.

Concrete design: at every site where a guard has been erased under this family
(recorded, per [[00-optimization-routine]]'s ledger discipline, in each accepted
erasure task), emit a debug-build-only verification instruction that recomputes the
*erased* check and asserts it would have passed — not a fallback path, a pure assertion
that panics with a precise diagnostic (which erasure task's proof, which site, what
value violated it) if the erased fact turns out false at runtime during development or
CI. This directly reuses the "check exists in debug, vanishes in release" shape every
one of this project's `#[cold]`/diagnostic paths already assumes rustc/LLVM will
correctly strip, verified rather than assumed per this project's standing discipline
(see [[102-reject-unrepresented-aot-relocations]]'s precedent of verifying, not
trusting, what the AOT pipeline actually emits).

Concrete steps:
1. Add a project-wide convention (a macro, consistent with this project's existing
   macro-generated stencil-family pattern) for "assert erased fact `X` at site `Y`,
   compiled to nothing in release" and wire it to reference the specific accepted
   erasure task by number in its panic message, so a debug-build failure points directly
   at which proof to re-examine.
2. Instrument every currently-accepted erasure site from [[231]]/[[233]]/[[235]]/[[236]]
   with this assertion.
3. Run the full correctness test corpus, [[237]]'s adversarial catalog, and the full
   V8v7 suite under a debug build with assertions enabled, and confirm zero assertion
   failures — this is the practical closing step that turns "we proved it" into "we
   proved it and nothing we can run disagrees."
4. Verify by disassembly (matching [[209]]'s and [[102]]'s existing discipline of
   checking emitted machine code rather than trusting source intent) that the release
   build genuinely contains zero instructions from this instrumentation — a debug-only
   assertion that accidentally survives into release would silently reintroduce the
   exact per-call check cost this whole family exists to eliminate.

Acceptance: every currently-accepted erasure site carries a debug-only verification
assertion; the full correctness corpus, [[237]]'s adversarial catalog, and the V8v7
suite run clean (zero assertion failures) under a debug build; disassembly of the
release binary confirms zero instructions attributable to this instrumentation at every
instrumented site; a deliberately-broken proof (a mutation test: temporarily invalidate
one site's precondition and confirm the debug-build assertion actually fires) is run at
least once per site family to confirm the harness itself is load-bearing, not merely
present.

Primary source: this is the standard `debug_assert!`/contract-checking pattern; no
external citation needed beyond this project's own existing zero-cost-in-release
discipline.
