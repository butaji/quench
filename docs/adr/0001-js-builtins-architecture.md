# ADR 0001: Self-hosted JS builtins on a small Rust core

- Status: accepted (confirmed 2026-08-08, grilling session)
- Context: `AGENTS.md` point 2 states the architecture is a small Rust core
  plus self-hosted JS builtins (`builtins/*.js`) calling only `__ops__`.
  An audit found zero JS builtins in the repo — all builtins are currently
  Rust (`crates/quench-runtime/src/builtins/*.rs`) —
  making the doc read as fiction.
- Decision: the JS-builtins architecture **is** the plan, not an abandoned
  idea. The governing rule (confirmed 2026-08-08): **everything that can be
  done in JS must be done in JS; the Rust core stays at the minimum possible
  size.** The "JS ≈ 1/3 of Rust LOC" figure in `docs/architecture.md` is a
  consequence of that rule, not a target in itself — Rust is expected to
  shrink as builtins migrate (existing Rust builtins get deleted per-stage).
- Enforcement rule (confirmed 2026-08-08, applies unconditionally):
  no new Rust builtins — anything implementable in JS lands in JS first;
  the 21 stages already done in Rust are migration debt to be repaid under
  R0, not final state.
- Mechanism: `%ops%` bridge (`eval/ops.rs` + `builtins/core/ops_wrapper.rs`,
  currently a scaffold) and `builtins/bootstrap.rs` (planned), per
  `tasks/refactor-plan.md` items R1 (PHASE-B) and R0 (PHASE-B).
  Note: `AGENTS.md` says `__ops__`; the actual name in code is `%ops%`.
- Consequences:
  - `AGENTS.md` and `docs/architecture.md` must label this as the *target*
    architecture until the first builtin actually moves to JS.
  - `tasks/index.json` `impl: "js"` / `js_loc` fields describe planned, not
    current, state.
  - Rules that only make sense post-migration (point 6 "JS-builtin fixes
    gated by the stage run alone", point 8 `__ops__` surface) are
    forward-looking policy and stay.
