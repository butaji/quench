# AGENTS.md

1. **Goal**: 100% test262 conformance, one stage at a time (122 stages, see `tasks/index.json`), with the minimum possible LOC.
2. **Architecture (target — R0/R1, not yet landed)**: small Rust core (`crates/quench-runtime`) plus self-hosted JS builtins (`builtins/*.js`) that only call `__ops__`; everything that can be implemented in JS **must** be implemented in JS on top of the Rust core (ADR `docs/adr/0001-js-builtins-architecture.md`). Today all builtins are Rust (`crates/quench-runtime/src/builtins/*.rs`); details in `docs/architecture.md`.
3. **Never modify** `tests/test262/` or `crates/quench-runtime/tests/test262.rs`; `test/intl402` and `test/staging` are out of scope.
4. **No TDD — tests are regression guards only**: never write a failing `#[test]` before implementing new behavior; the test262 stage run is the correctness gate. Write a Rust unit test **only** when fixing a bug (reproducer that stays in), pinning a core invariant test262 cannot observe, or pinning a refactor. No debugging, no `println!`, no guessing, no speculative edits.
5. **Unit tests live only in the Rust core** — regression guards for complicated issues (bug reproducers, core invariants test262 cannot observe, refactor pins); never replicate a test262 assertion as a unit test.
6. **Fix workflow**: run test262 stages in batch and fix families of related failures, not single cases; a Rust-core fix is reproduce → watch fail → minimal fix → re-run test and stage → leave the test in; JS-builtin fixes are gated by the stage run alone.
7. **Linter is law**: zero clippy warnings (`-D warnings` is set); every `*.rs`, `*.ts`, `*.js` file in this repo: max 500 lines/file, 40 lines/function, cognitive complexity ≤ 10; no `#[allow]` exceptions.
8. **One canonical spec-op path**: `ToPrimitive`, `IsCallable`, `SameValueZero`, etc. live only in `src/eval/ops.rs`, exposed to JS as `__ops__`; grep before writing any helper.
9. **Prefer crates over hand-rolling** (`regress`, `chrono`, `num-bigint`, `serde_json`, `urlencoding`, `oxc`); a new crate needs a `docs/DEPENDENCIES.md` row in the same diff.
10. **Zero duplication, zero dead code, no speculative generality**: hoist repeated logic to `value/` or `eval/ops.rs`; delete unused symbols, fields, and variants in the same PR.
11. **Builtins throw, never panic**: use `value::error::throw_type_error(msg)`; `panic!`/`unwrap()`/`expect()` are forbidden in `builtins/` and `eval/`.
12. **Verify before done**: unit test green, module suite green, test262 stage green, `cargo fmt` + `cargo clippy -p quench-runtime --all-targets` clean.
13. **Conventions**: symbol payloads are raw `desc\0id` strings used directly as property keys; boxed primitives live in a `_value` property; strictness is captured at definition site; accessors via `Object::define_accessor`; `CURRENT_CONTEXT` is a thread-local raw pointer during eval.
14. **Progress SSOT is the test262 run**: never record pass/fail counts, pass rates, or test totals in `docs/` or `tasks/*.md`; run the stage to know where you stand. `tasks/index.json` holds only stage identity and workflow status (`status`, `current_stage`), updated by `tools/advance-stage.sh` from actual runs.

## Commands

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime
cargo clippy -p quench-runtime --all-targets

# Run current stage (TEST262_STAGE=N for a specific one, ALL_STAGES=1 for all)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# Diagnostics: cargo run --bin run-test -- <test.js>; TEST262_DIGEST=1 for grouped failures
```
