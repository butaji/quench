# Task 68: Add test262 submodule and conformance harness

## Goal

Add the official ECMAScript test262 suite (`https://github.com/tc39/test262`) as a Git submodule and create a Rust harness that runs selected tests against `quench-runtime`. Use the failures found to drive focused unit-test regression fixes.

## Design

See `docs/superpowers/specs/2026-06-28-test262-conformance-design.md`.

## Implementation plan

See `docs/superpowers/plans/2026-06-28-test262-conformance-plan.md`.

## Scope (first phase)

- Run only tests that do **not** require: ES modules, async/Promise, strict-mode enforcement, generators, classes, Symbol, RegExp, BigInt, Proxy, Reflect, WeakMap/WeakSet, TypedArray, spread, `for...of`, `do...while`, `delete`, unary `+`, optional chaining, tagged templates, private names, JSX.
- Start with `test/language/expressions/` and `test/built-ins/Array/`.
- Skip unsupported features/flags explicitly.

## Out-of-scope

- `module` flag tests.
- `async` flag / Promise tests.
- `onlyStrict` tests until strict mode is enforced.
- Tests requiring unimplemented built-ins.

## Files to create/modify

- `.gitmodules` — add `tests/test262` submodule.
- `crates/quench-runtime/Cargo.toml` — dev-deps `walkdir`, `serde_yaml`.
- `crates/quench-runtime/src/test262/metadata.rs` — frontmatter parser.
- `crates/quench-runtime/src/test262/harness.rs` — minimal assert/sta/$DONE helpers.
- `crates/quench-runtime/src/test262/runner.rs` — skip policy, execution, reporting.
- `crates/quench-runtime/src/test262/mod.rs` — module entry.
- `crates/quench-runtime/src/lib.rs` — expose test262 module under test cfg.
- `crates/quench-runtime/tests/test262.rs` — integration test entry.
- `xtask/src/main.rs` — `test-test262` command.
- `scripts/run_tests.sh` — dispatch command.
- `docs/test262.md` — usage docs.
- `tasks/index.json` — track this task and any regression-fix subtasks.

## Verification

```bash
git submodule update --init tests/test262
./scripts/run_tests.sh test-test262
cargo test -p quench-runtime
```

All commands must run with timeouts (see Task 31).
