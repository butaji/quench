# 45 — No-tier first execution: near-optimized code on first run

Status: planned

Turn the "skip tiering" advantage into a concrete build step rather than an architectural claim. Currently [[03-stencil-only-execution]] already compiles on first use with no hotness threshold, but the guard/IC machinery being planned in [[19-guard-typed-connectors]], [[25-generalized-speculative-guards]], and [[31-bounded-polymorphic-guards]] must land in the *first* compilation path, not a second optimizing pass — otherwise the VM quietly grows its own baseline/optimized split and the structural advantage over V8/JSC tiering is lost by accident.

Resolution vs. [[22-osr-tier-transition]]: this task is the default architectural bet, not one option among several — 22 is demoted to an optional fallback, to be built only if measurement shows this single-tier approach has a real ceiling for long-running loops that [[48-submorphism-repatch-on-guard-miss]] cannot reach cheaply. Treat any reintroduction of a second compilation tier as a regression against this task's acceptance criteria unless 22's own fallback trigger condition is documented as met.

Concrete steps:
1. Audit every planned guard/IC task (19, 25, 31, 32) and confirm its target integration point is the single first-use compilation path in `dynjit.rs`/`main.rs`, not a hypothetical "tier 2."
2. Add a coverage-report check (extending [[04-bytecode-coverage-map]]) that fails if any opcode requires a second compilation of the same function to reach `INLINE_STENCIL` status.
3. Benchmark short-lived programs (process-lifetime-bounded scripts, not the steady-state V8v7 loop suites) where V8 never leaves Ignition/Sparkplug, and record a comparison showing time-to-first-optimized-instruction.

Acceptance: no function in the test suite requires more than one compilation to reach its fully-guarded, fully-specialized form; a documented A/B comparison exists specifically for startup-dominated workloads (not the steady-state V8v7 suite, which favors long-running tiered JITs by construction).
