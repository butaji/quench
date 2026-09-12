# 166 — IC recipe health scoring

Status: planned

Add a normalized, low-overhead health report for property, computed-property, call, and
arithmetic IC sites. The report records recipe identity, state, arm count, successful
arms, fallback executions, helper calls, prototype depth, materializations, and an
explicit static/dynamic cost score. Aggregate by recipe algebra form and bytecode family,
never by benchmark identity or source spelling.

Use zero-overhead disabled instrumentation and a diagnostic build mode, following the
existing residual block profiler. The optimization routine consumes the report to select
the highest total-cost unsupported recipe. This makes coverage work data-driven while
preserving the rule that ordinary execution always uses stencils and compilation never
waits for hotness.

Acceptance: disabled-mode disassembly is unchanged; enabled counters reconcile with IC
state transitions; reports identify known forced-fallback tests; a script emits a ranked
machine-readable candidate list and links it to the task ledger.

Source: <https://firefox-source-docs.mozilla.org/js/cacheir.html>.

## Round-thirty-nine feedback use-def census

Task 393 must collect, without disabling the native path, a use-def record for mutable
feedback: allocated field, writes, consuming recipe reads, invalidations, and final state.
Derive the report by semantic fact kind and recipe identity. In particular, distinguish
never-written, written-and-never-read, statically predictable, and read-by-surviving-code.

Task 392 consumes this census to construct the demand-projected feedback plane. This task
does not independently remove slots or invent another feedback layout. The 2026 feedback
study reports substantial unused recording in another dynamic-language compiler; use that
as a hypothesis to test locally, not as an assumed ratio:
<https://doi.org/10.4230/LIPIcs.ECOOP.2026.16>.
