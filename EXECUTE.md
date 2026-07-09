> **Execution contract for the Quench runtime.**
>
> Run sub-agents in parallel to move faster on independent tasks.
> No comments, no guessing, tdd
> Linter rules. File: 500 lines max, function: 40 lines max, 10 complexity max. All *.rs files, not exceptions.
> Commit and push after each successful step

# Goal

Reach **100% compatibility with JavaScript, TypeScript, TSX, and JSX** in `crates/quench-runtime/`, executing `.ts/.tsx/.js/.jsx` natively, with the **minimum amount of code**, the **maximum possible performance**, and **complete coverage of fast Rust unit tests** so every spec behavior is verified and regressions are caught immediately. Keep it Ink-compatible.

## Current priority — drive test262 (in order)

The build is green again (Task 330 done). Most test262 cases are **skipped on missing
harness helpers**, not failing on language bugs. Work this sequence, one verified step
at a time, before any other feature:

1. **Un-stub `assert.compareArray` / `assert.arrayContains` — Task 357 (start here).**
   Stubs returning `undefined` at `crates/quench-runtime/src/test262/harness.rs:173-174`;
   violates Principle 7 (No stubs). SameValue semantics (NaN == NaN, +0 != -0). Unblocks 358.
2. **Expand the harness include allowlist — Task 358 (blocked by 357).** Inline slice at
   `crates/quench-runtime/src/test262/batches.rs:47` (currently 3 files). Add
   `propertyHelper.js`, `nativeErrors.js`, `deepEqual.js` first. Measure:
   `cargo test --test test262 -- --ignored 2>&1 | grep "unsupported include" | sort | uniq -c | sort -rn`.
3. **Then one directory at a time** via the incremental loop (Task 355,
   `docs/incremental-conformance-workflow.md`).

**Do not re-add or re-complete removed tasks.** Reopened to TODO (not actually implemented)
— do not mark complete without a verifying test: 97 (negative-test matching not wired into
the runner), 253 (harness_loader absent), 305 (rest params not collected into an array),
306 (linter enforces file-length only; `#[allow]` exemptions present), 350 (no per-iteration
for-of `Let` rebinding).

**Task hygiene (every iteration):** verified done (cite test/commit) → remove
`tasks/<id>-*.md`; `COMPLETED` but not verifiable against current code → set `TODO`. Keep
`tasks/` lean. Regenerate `tasks/index.json` with `python3 scripts/target_tasks.py`.

## Principles

1. **High impact, low effort first.** Every decision is filtered by effort vs. payoff. Prefer the change that fixes the most failures, unblocks the most examples, or removes the biggest stability risk with the smallest patch.
2. **Reuse before rewriting.** Prefer crates and stdlib. Mandatory crates: `swc`, `serde_json`, `regress`, `miette`/`ariadne`, `lasso`, `indexmap`, `num-bigint`/`rust_decimal`, `bumpalo`, `rustc-hash`/`foldhash`, `thiserror`.
3. **80/20 Pareto.** Unblock examples and conformance tests first.
4. **No cross-compilation / no transpilation.** Execute `.ts/.js/.tsx/.jsx` natively via `swc` parsing and lowering. No `esbuild`, `tsc`, or Deno step, and no `import`/`export` stripping.
5. **Strict build-time linting.** Max 500 lines/file, 40 lines/function, complexity 10. Applies to every `*.rs` file in the workspace, including the Rust code that implements JS/TS/TSX/JSX semantics; no `#[allow(...)]` or file exemptions.
6. **Spec-compliant implementation.** JS/TS/TSX/JSX behavior must match ECMA-262, the TypeScript language spec, and the JSX spec. Verify gaps via test262 / TypeScript harnesses and focused Rust unit tests.
7. **No stubs.** If a feature is not implemented, the runtime must throw a clear error or panic. Do not silently return `undefined`, no-op, or use placeholder behavior.
8. **Granular, test-driven development.** Every bug fix and feature starts with a failing unit test. Each test must be small, isolated, and named after the behavior it protects. Prefer `#[test]` units over broad integration tests; a regression must be reproducible by running a single test name. The project must have complete coverage of fast Rust unit tests for every spec behavior.
9. **Parallel sub-agents.** Dispatch independent exploration, planning, and implementation work to sub-agents running in parallel. Use the decision filter and task tracker to keep work aligned and avoid conflicts.
10. **Minimum code, maximum performance.** Every feature is implemented with the smallest Rust surface that fully matches the spec. Avoid speculative abstractions, layers, and generic wrappers. Optimize the hot path; keep cold paths simple.
11. **Document deferrals.** Postponed features and design decisions must be written down.

## Testing policy

- One behavior, one test. A unit test should fail for exactly one reason.
- Tests live next to the code they exercise (`crates/quench-runtime/tests/` for runtime behavior, inline `#[cfg(test)]` modules for internal data structures).
- Every fix or feature must list the specific test file(s) or test name(s) added/modified in its acceptance criteria.
- Before claiming work complete, run:
  ```bash
  cargo test -p quench-runtime <test_name>
  ```
  and confirm the test fails before the fix and passes after.
- Conformance gaps are tracked with test262 / TypeScript harness tests, but each fix must also have a focused Rust unit test that isolates the failure.
- **JS/TS scenario tests.** In addition to pure-Rust unit tests, exercise runtime behavior through real JavaScript and TypeScript snippets evaluated by the runtime. These scenarios live in `crates/quench-runtime/tests/scenarios/` as `.js`/`.ts` files paired with Rust harness tests that assert on the evaluated result, console output, or thrown error.

## Decision filter

Before starting any task, rank it:

| Priority | Action |
|----------|--------|
| High impact + low effort | Do immediately. |
| High impact + high effort | Plan and split into smaller low-effort steps. |
| Low impact + low effort | Batch or defer. |
| Low impact + high effort | Do not do. |

## Boundaries

Do not touch:

- `src/bridge/`, `src/ink/`, `src/render/`
- `examples/`, `tests/test262/`, `tests/typescript/`

Allowed:

- `crates/quench-runtime/src/`
- `src/main.rs` for host-function registration
- `src/event_loop.rs` for JS dispatch
- `src/runtime.js` for compatibility shims

## Verification

```bash
cargo check
timeout 120 cargo test -p quench-runtime
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

## Conformance

See `docs/conformance.md` for running the test262 and TypeScript harnesses.
