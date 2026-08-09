# AGENTS.md

1. **Goal**: 100% test262 conformance, one stage at a time (see `tasks/index.json`), with the minimum possible LOC — hard ceiling ~100k handwritten Rust LOC for the engine. The only architectural authority is ADR `docs/adr/0005-oxc-facts-residual-vm.md`; a baseline compiler is deferred and, if ever added, consumes the same residual Ops.
2. **Architecture (only plan)**: follow `docs/architecture.md` and ADR 0005. OXC owns AST, scopes, and symbols; Quench queries that data plus a unified `ProgramDb` fact algebra (`Proven`, `Guarded`, `Unknown`) and reduces programs to residual ops. Do not build a second AST, HIR/MIR ladder, TypeGraph, self-hosted-JS builtin layer, or alternate semantic pipeline. The reducer has five contexts (`value`, `place`, `effect`, `control`, `define`) and a small semantic kernel; `ops!`/`heap!`/`builtin!` declarations generate only mechanical metadata. Preserve observable JS behavior before specializing. Heap references are compact `HeapRef(u32)` indices; shapes/slots and continuations are the runtime representation. Complex builtin algorithms are readable Rust; primordials are static generated data.
3. **Never modify** `tests/test262/` or `crates/quench-runtime/tests/test262.rs`; `test/intl402` and `test/staging` are out of scope. The "100% test262" claim is pinned to the submodule commit recorded in `tasks/index.json` (`test262_pin`).
4. **No TDD — tests are regression guards only**: never write a failing `#[test]` before implementing new behavior; the test262 stage run is the correctness gate. Write a Rust unit test **only** when fixing a bug (reproducer that stays in), pinning a core invariant test262 cannot observe, or pinning a refactor. No debugging, no `println!`, no guessing, no speculative edits.
5. **Unit tests live only in the Rust core** — regression guards for complicated issues (bug reproducers, core invariants test262 cannot observe, refactor pins); never replicate a test262 assertion as a unit test.
6. **Fix workflow**: run test262 stages in batch and fix families of related failures, not single cases; a Rust-core fix is reproduce → watch fail → minimal fix → re-run test and stage → leave the test in; JS-builtin fixes are gated by the stage run alone.
7. **Linter is law**: zero clippy warnings (`-D warnings` is set); every `*.rs`, `*.ts`, `*.js` file in this repo: max 500 lines/file, 40 lines/function, cognitive complexity ≤ 10; no `#[allow]` exceptions.
8. **One canonical semantic path**: `ToPrimitive`, `IsCallable`, `SameValueZero`, etc. have one semantic owner; residual ops and specializations delegate to it. Grep before writing any helper. Never optimize through proxies, accessors, coercion, `Symbol.toPrimitive`, dynamic prototype mutation, direct `eval`, realms, or completion ordering.
9. **Prefer crates over hand-rolling** (`regress`, `chrono`, `num-bigint`, `serde_json`, `urlencoding`, `oxc`); a new crate needs a `docs/DEPENDENCIES.md` row in the same diff.
10. **Zero duplication, zero dead code, no speculative generality**: hoist repeated logic to `value/` or `eval/ops.rs`; delete unused symbols, fields, and variants in the same PR.
11. **Builtins throw, never panic**: use `value::error::throw_type_error(msg)`; `panic!`/`unwrap()`/`expect()` are forbidden in `builtins/` and `eval/`.
12. **Verify before done**: unit test green, module suite green, test262 stage green, `cargo fmt` + `cargo clippy -p quench-runtime --all-targets` clean.
13. **Conventions**: symbol payloads are raw `desc\0id` strings used directly as property keys; boxed primitives live in a `_value` property; strictness is captured at definition site; accessors via `Object::define_accessor`; `CURRENT_CONTEXT` is a thread-local raw pointer during eval.
14. **Progress SSOT is the test262 run**: never record pass/fail counts, pass rates, or test totals in `docs/` or `tasks/*.md`; run the stage to know where you stand. `tasks/index.json` holds only stage identity and workflow status (`status`, `current_stage`), updated by `tools/advance-stage.sh` from actual runs.
15. **Commit and push as you progress** (`GOAL.md`): small commits as work lands; never leave a finished, verified step uncommitted.

## Commands

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime
cargo clippy -p quench-runtime --all-targets

# Run current stage (TEST262_STAGE=N for a specific one, ALL_STAGES=1 for all)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# Diagnostics: cargo run --bin run-test -- <test.js>; TEST262_DIGEST=1 for grouped failures
```
