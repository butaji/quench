# 302 — Quantify the legacy/generality tax in V8/JSC's own hot functions

Status: planned

Companion to [[254-v8-jsc-source-profile-cross-reference]], adding the specific lens
this session's "how do we build something more efficient and modern, not just a clone"
question needs: [[254]]'s disposition scheme ((a) already equivalent, (b) planned
equivalent, (c) new gap) tells you what to *copy*. This task asks the complementary
question — for each hot V8/JSC function [[253]]/[[254]] identified, how much of its
actual instruction count is the *core algorithm* versus overhead that exists only
because V8/JSC must serve requirements this project does not have: multi-realm/
multi-context isolation (browser tabs, iframes, workers sharing one process), embedder
callback hooks (a JS engine embedded in Chrome/Node must call out to host code at many
points a standalone VM never needs to), 32-bit-pointer-era value representation
constraints V8's `Smi`/pointer-compression heritage still carries even on 64-bit
targets, x86-first codegen assumptions that don't bind an AArch64-only project, ABI
stability across V8/JSC releases (embedders pin API versions; nothing here does), and
decades of incrementally-patched security/spec-compliance edge cases for a general-
purpose web engine, not a benchmark-focused closed-world VM.

**This produces two distinct outputs [[254]] alone does not:** a list of "yes, copy
this — it is core-algorithm cost, not legacy" items (strengthens the case for adopting
that specific mechanism), and a list of "no, do not copy this pattern — it is legacy
tax this project should never pay in the first place because the feature that
necessitates it doesn't exist here" items (an argument for *deliberately diverging* from
V8/JSC's design at those specific points, not converging toward it).

Concrete steps:
1. For each hot symbol already read under [[254]] (`montReduce`, `bnpSquareTo`,
   `Scheduler.schedule`'s surrounding dispatch machinery, `Planner.incrementalAdd`, and
   any others [[254]]'s ongoing work adds), classify its instructions/logic into: core
   algorithm, multi-realm/context-check overhead, embedder-hook overhead, legacy
   value-representation overhead, x86-specific-then-ported overhead, or
   spec-compliance-edge-case overhead — with a citation to the specific source lines
   supporting each classification, not a guess.
2. For any function where legacy/generality overhead is a measurable fraction, state
   explicitly whether this project's closed-world, single-realm, AArch64-only,
   Rust-ownership design already structurally avoids that category of cost (verify, do
   not assume) or would need a deliberate design decision to avoid it.
3. Produce the two-list output explicitly: mechanisms worth adopting because they are
   core-algorithm cost, and mechanisms worth explicitly *not* adopting because they are
   legacy tax this project's different constraints make unnecessary.

Acceptance: at least the crypto and richards/deltablue hot-symbol sets from [[253]] have
this classification applied with source citations; the two-list output (adopt vs.
deliberately diverge) is produced and cross-referenced against the relevant existing
tasks in both directions — a "worth adopting" finding points at the task that would
implement it (if none exists, a new one is opened per [[254]]'s own precedent), a
"deliberately diverge" finding points at whichever existing task ([[252]]'s single-file
scope, [[51]]'s legacy-free representation audit) already benefits from the explicit
justification.

Source: [[253]]'s and [[254]]'s existing artifacts and source reading; no new external
citation beyond V8/JSC's own source, already in scope for [[254]].
