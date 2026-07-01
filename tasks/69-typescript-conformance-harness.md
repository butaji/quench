# Task 69: Run TypeScript submodule conformance from Rust

## Goal

Replace the Node-based `scripts/run_typescript_runner.sh` with a Rust conformance harness that runs TypeScript test cases from `tests/typescript` against `quench-runtime`. Start with baseline-JS mode; add source-direct TS mode once `Context::eval_ts` is implemented.

## Design

See `docs/superpowers/specs/2026-06-28-unified-conformance-harness-design.md`.

## Implementation plan

See `docs/superpowers/plans/2026-06-28-unified-conformance-harness-plan.md`.

## Scope

### Phase 1 — baseline JS

- Walk `tests/typescript/tests/cases/conformance/`.
- Parse directives (`@target`, `@module`, `@jsx`, `@filename`, `@noEmit`, `@emitDeclarationOnly`, etc.).
- Skip unsupported cases per `docs/conformance.md`.
- Load pre-compiled `.js` baselines from `tests/typescript/tests/baselines/reference/`.
- Register TS emit helpers (`__extends`, `__assign`, `__awaiter`, `__decorate`, `__importStar`, `__importDefault`) as Rust native functions.
- Evaluate with `ctx.eval()` and report to `target/conformance_report.json`.

### Phase 2 — source-direct TS

- Implement `Context::eval_ts` using `swc_ecma_parser` + `swc_ecma_transforms_typescript::strip`.
- Try original `.ts`/`.tsx` source directly.
- Support multi-file cases via `// @filename:` splitting.
- Add hybrid mode: source-direct first, baseline fallback.

## Files to create/modify

- `crates/quench-runtime/Cargo.toml` — swc transform deps.
- `crates/quench-runtime/src/swc_parse.rs` — `parse_ts`.
- `crates/quench-runtime/src/lib.rs` — `eval_ts` / `eval_ts_module`.
- `crates/quench-runtime/src/conformance/mod.rs` — shared module.
- `crates/quench-runtime/src/conformance/report.rs` — shared report types.
- `crates/quench-runtime/src/conformance/typescript/mod.rs` — runner.
- `crates/quench-runtime/src/conformance/typescript/directives.rs` — directive parser.
- `crates/quench-runtime/src/conformance/typescript/baseline.rs` — baseline lookup.
- `crates/quench-runtime/src/conformance/typescript/helpers.rs` — native emit helpers.
- `crates/quench-runtime/src/conformance/typescript/skip.rs` — skip rules.
- `crates/quench-runtime/tests/conformance.rs` — unified entry.
- `xtask/src/main.rs` — `test-conformance` command.
- `scripts/run_tests.sh` — dispatch.
- `docs/conformance.md` — update to Rust harness.
- `tasks/index.json` — track this task and regression-fix subtasks.

## Verification

```bash
git submodule update --init tests/typescript
./scripts/run_tests.sh test-conformance
cargo test -p quench-runtime
```

All commands must run with timeouts (see Task 31).
