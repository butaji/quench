# Strategy — execution order

`quench-runtime` is a pure JavaScript engine (`OXC AST → Quench IR →
interpreter`); `quench-test262` is a separate conformance client. No
runner-specific behavior belongs in the engine.

End-state shape (small Rust core + self-hosted JS builtins) is fixed in
`docs/architecture.md`; the work queue lives in `tasks/refactor-plan.md`;
stage status lives in `tasks/index.json`. **This file decides only the
execution order** and records rejected approaches.

Speed = fixes-per-week × tests-unlocked-per-fix. Rank every lever by that.

## Critical path

```
Phase A — clear language stages
  A1. R5 object-model correctness (partial done; unblocks Object + eval)
  A2. Fix each stage by S2 digest clusters, not per-test
  A3. R17 oxc_semantic early errors (high tests/LOC)
  A4. S8 url over urlencoding (before stage 53 modules)
  A5. Grow R1 only for ops you actually touch

Phase B — before grinding built-ins (stage 71 Object)
  B1. Finish R1 (__ops__ owns impls, not re-exports)
  B2. R0 self-host builtins in JS (Object first, dependency order)
  B3. R2 one iterator protocol (with R0 Iterator.js)
  B4. R3 chrono for Date core

Phase C — built-ins → annexB → Temporal
  C1. Built-ins stages in JS (never re-expand Rust builtins)
  C2. S4 async→generator (for-await-of / Promise / Async*)
  C3. R18 regex Unicode escapes (before stage 84)
  C4. Temporal last (temporal_rs + ICU4X; stage 120)
```

## S1 — Object model + digest before full R0

Language-stage failures share one property store: finish R5 first. Full
R1→R0 pays off in the built-ins half — schedule it as Phase B. Do not
grind Object/Array/String in Rust; that work dies under R0.

## S2 — Fix by root cause, not by test

Per stage: digest → group failures by error/op/missing intrinsic → one
reproducer `#[test]` for the dominant cluster → fix → re-digest. Prefer
root causes that also help later stages. Tooling: `docs/tools.md`.

## S3 — OXC early errors via `oxc_semantic`

A large slice of language tests are static-semantics early errors. OXC
implements them; hand-rolling in `lower/` is thousands of LOC. Route
parse → `oxc_semantic` → SyntaxError before lowering (R17).

## S4 — Async-to-generator transform

Generators already pass. Async stages reduce to generators + a job queue
if the transform runs at lower time. Confirmed (2026-07-23):
`oxc_transformer` has **no** async-to-generator transform;
`swc_ecma_compat_es2017::async_to_generator` does, but pulls in the swc
stack alongside `oxc`. Verify standalone usability and subset-test
`for-await-of` before committing; if swc conflicts, hand-roll async eval
nodes in `eval/`.

## S5 — Parallel in-stage runner + digest tooling

Landed (2026-07-23): parallel digest, explicit skips, JSON digests,
failed-only rerun, QUICK triage, prebuilt `run-test` isolation. Next:
runner tooling items in `tasks/019-runtime-boundaries.md`.

## S6 — Disciplined unit tests

Per `AGENTS.md`: reproducers, core invariants, refactor pins only. Never
duplicate test262 assertions as unit tests.

## S7 — Crate-first for every remaining primitive

Policy: `docs/DEPENDENCIES.md`. Long pole: `Temporal` — staged last.
`temporal_rs` confirmed production-grade (Boa, Kiesel, V8/Chrome 144;
ES Stage 4). Add its `docs/DEPENDENCIES.md` row when the stage starts.

## S8 — `url` over `urlencoding`

`urlencoding` only does `%`-encoding; ES modules need full URL Standard
resolution. Replace with the `url` crate before stage 53 (`modules`).

## Rejected / low value

- *Full R0 before finishing language stages* — schedule as Phase B.
- *R15 file-split sweeps ahead of failing clusters* — lint gate stays
  enforced on touched code; wholesale splits unlock ~0 tests.
- *Parallel stage execution* — hides root causes; use in-stage
  parallelism (S5).
- *Coverage-driven unit tests* — see S6.
- *Per-stage checkpoint/skip lists* — a skip list is a lie that compounds.
- *Grinding Object/Array/String in Rust* — deleted by R0.
- *oxc_transformer for async* — no async-to-generator (see S4).
- *tokio for async* — overkill for a microtask queue.
- *fancy-regex / re2 for RegExp* — too limited for ES2024 `\p{}` (R18).
- *wasmtime for ShadowRealm* — it is a JS-level isolated global, not WASM.

## CI regression gate

Run `ALL_STAGES=1` on every merge to `main`; a previously-done stage
regressing blocks the merge.
