# AGENTS.md

1. **Goal**: build the frozen OXC-facts residual-VM architecture from scratch with minimum handwritten LOC.
2. **Architecture (only plan)**: OXC owns AST, scopes, and symbols; Quench queries that data plus unified `ProgramDb` facts (`Proven`, `Guarded`, `Unknown`) and reduces programs to residual ops. Do not build a second AST, HIR/MIR ladder, TypeGraph, self-hosted-JS builtin layer, or alternate semantic pipeline. Use the five reducer contexts (`value`, `place`, `effect`, `control`, `define`) and a small semantic kernel. Preserve observable JS behavior before specializing. Keep compact `HeapRef(u32)` references, shapes/slots, and continuations as runtime foundations.
3. **Test runner ownership**: `crates/quench-test262` owns test262 metadata, harness composition, and runner contracts. Never put test262 runner code in `crates/quench-runtime`; never modify `tests/test262/`.
4. **No TDD — tests are regression guards only**: never write a failing `#[test]` before implementing new behavior. Write a Rust unit test only for a bug reproducer, a core invariant the runner cannot observe, or a refactor pin. No debugging, no `println!`, no guessing, no speculative edits.
5. **Unit tests live only in the Rust core** — regression guards for complicated issues (bug reproducers, core invariants test262 cannot observe, refactor pins); never replicate a test262 assertion as a unit test.
6. **Fix workflow**: fix families of related failures, not single cases; reproduce → minimal fix → re-run relevant tests → leave the regression guard in.
7. **Linter is law**: zero clippy warnings (`-D warnings` is set); every `*.rs`, `*.ts`, `*.js` file in this repo: max 500 lines/file, 40 lines/function, cognitive complexity ≤ 10; no `#[allow]` exceptions.
8. **One canonical semantic path**: `ToPrimitive`, `IsCallable`, `SameValueZero`, etc. have one semantic owner; residual ops and specializations delegate to it. Grep before writing any helper. Never optimize through proxies, accessors, coercion, `Symbol.toPrimitive`, dynamic prototype mutation, direct `eval`, realms, or completion ordering.
9. **Prefer crates over hand-rolling** (`regress`, `chrono`, `num-bigint`, `serde_json`, `urlencoding`, `oxc`); a new crate needs a `docs/DEPENDENCIES.md` row in the same diff.
10. **Zero duplication, zero dead code, no speculative generality**: hoist repeated logic to `value/` or `eval/ops.rs`; delete unused symbols, fields, and variants in the same PR.
11. **Builtins throw, never panic**: use `value::error::throw_type_error(msg)`; `panic!`/`unwrap()`/`expect()` are forbidden in `builtins/` and `eval/`.
12. **Verify before done**: relevant unit tests green, `cargo fmt`, and `cargo clippy --workspace --all-targets -- -D warnings` clean.
13. **Conventions**: symbol payloads are raw `desc\0id` strings used directly as property keys; boxed primitives live in a `_value` property; strictness is captured at definition site; accessors via `Object::define_accessor`; `CURRENT_CONTEXT` is a thread-local raw pointer during eval.
14. **No stale progress tracking**: do not add stage ledgers, pass-rate reports, or task status files. Test runs are ephemeral verification output.
15. **Commit and push as you progress** (`GOAL.md`): small commits as work lands; never leave a finished, verified step uncommitted.

## Commands

```bash
cargo build -p quench-runtime
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
