# 252 — Validate the unstated single-file scope boundary of the closed-world/whole-program thread

Status: planned

**Architectural-flaw hypothesis:** a large and growing fraction of this project's most
ambitious work — [[46-closed-world-mode]], [[69-supercompilation-colimit]],
[[219-js-to-native-lowering-functor]], [[223-agt-derived-guard-synthesis]],
[[231]]–[[236]]'s exactness-erasure family, [[239-occurrence-typed-proven-pipeline]],
[[240-fibered-kernel-selection-over-shape-lattice]], and [[242-whole-program-0cfa-precompilation-pass]]
— all rest on a "whole program" being available and closed at compile time. Every one
of the V8v7 corpus's eight suites *is* exactly that: one self-contained `.js` file per
suite, with no imports, no separate compilation units, no code arriving after the
initial parse. **No task in this project's ledger states this as an explicit,
acknowledged scope boundary**, and none validates what currently happens if this VM is
given a realistic multi-file program (multiple `<script>` tags, a bundler-free ES module
graph, or any input where "the whole program" is not literally one parse). If the
closed-world machinery silently assumes single-file scope without checking it, a
multi-file program could either (a) be correctly and safely rejected/degraded to the
per-function fallback path, which would be fine, or (b) silently apply whole-program
optimizations to what it *thinks* is the whole program but is actually only one module
of a larger one — which would be an unsound optimization, not merely a missed one,
since a shape/call-target fact proven "closed" against an incomplete view of the real
program is not actually closed.

This matters beyond a theoretical concern: [[46]]'s own text already states it "rejects
or falls back to per-function analysis when `eval`, `Function` constructor, or dynamic
`load()` calls are present" — but does not mention multi-file/multi-module input at all,
suggesting this specific failure mode was not considered when [[46]]'s closed-world
detection was designed, distinct from the `eval`/`Function`-constructor cases it does
handle.

Concrete steps:
1. Determine directly, by reading [[46]]'s actual implementation (once it exists) or its
   current design text (if not yet implemented), whether "the whole program" is defined
   as "everything in this one parse" or something broader — and whether this VM's
   actual entry points (`cargo run -- path/to/script.js`, per the README) ever admit
   more than one script file as input in the first place. If the VM's own execution
   model genuinely only ever accepts one file (no `load()`-based multi-file composition
   the closed-world machinery would need to account for), state that explicitly as the
   resolution — the hypothesis would then be *refuted by design*, not merely untested,
   and every dependent task's scope is confirmed sound as-is.
2. If the VM *does* support multi-file composition (the README mentions a `load`
   host function, which [[46]]'s text references as something closed-world mode
   explicitly rejects when used dynamically) — confirm directly whether a `load()` call
   is always treated as breaking closed-world eligibility (safe) or whether there is any
   path where a `load()`-composed multi-file program could still reach a whole-program
   optimization pass under an incomplete view.
3. Add an explicit, tested boundary case: a program using `load()` to compose two files,
   each independently appearing "closed" in isolation, confirming the combined program
   is correctly treated as open-world (or correctly and soundly merged as one true whole
   program, if that is the actual design) rather than each file being unsoundly
   optimized against its own partial view.

Acceptance: the single-file-vs-multi-file scope question has a definitive, documented
answer (either "the VM's execution model makes this moot by construction" or "here is
the exact mechanism that keeps multi-file composition sound"), not left as an implicit
assumption; a concrete test exercises the `load()`-composition boundary case from step 3
and confirms no unsound whole-program optimization is ever applied to an incomplete
view; every dependent task in the closed-world thread ([[46]], [[69]], [[219]], [[223]],
[[231]]–[[236]], [[239]], [[240]], [[242]]) has this task's resolution noted as their
shared scope-validation dependency, so future readers do not have to re-derive whether
the assumption holds.

No external primary source needed — this is a direct audit of this project's own
execution model and closed-world implementation against a scope question its existing
text does not address.
