# Quench mission

Finish the architecture and every implementation task described by the
repository docs. Read `README.md`, `docs/`, and all of `tasks/` first; treat
them as the specification and keep their design coherent as the code evolves.

Build one OXC-facts residual VM: syntax and static meaning come from OXC,
facts describe what is proven/guarded/unknown, and runtime code represents only
the remaining dynamic uncertainty. Do not create parallel semantic worlds.

Think in Lisp:

- Make programs, semantics, state machines, reducers, and runtime operations
  uniform data wherever possible.
- Describe repeated mechanics once, then derive them with tables or Rust
  macros; do not hand-copy variants.
- Prefer composition and transformation of data over ad-hoc control flow.
- When failures repeat, improve the underlying representation or transition
  system instead of adding cases.

Work as a learning loop. Measure the current behavior, form a mechanism-level
hypothesis, implement the smallest general change, and attack it with nearby
and adversarial cases. Keep useful diagnostics and explanations, discard
temporary scaffolding, and leave the next worker with the clearest remaining
uncertainty. Choose whatever tools or experiments improve understanding; this
goal constrains outcomes, not the path.

Advance test262 in the order defined by `docs/STAGES.md`. Every discovered test
in stages 0–113 must execute through the canonical runner and pass, with no
skips or altered harness behavior. After each change, rerun the affected stage
and regression stages 0 through the current stage. Verify the complete range
again before claiming completion. Keep test262 itself an immutable input.

Completion requires source-level task coverage, 100% stable-stage results,
green workspace quality checks, and committed verified changes. Do not claim
success from a partial run, a workaround, or an untested assumption.
