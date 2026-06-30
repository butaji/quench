# Task 10: Architecture hardening — split builtins and guard recursion

## Goal

Keep the runtime architecture healthy as it grows.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/builtins/`
- `crates/quench-runtime/src/interpreter/`
- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/lower/`

## Done ✓

- ✅ **Recursion guard** is implemented in the interpreter (depth limit of 50000 with a clear `JsError::StackOverflow`).
- ✅ **`lower.rs` split into submodules**:
  - `lower/mod.rs` (47 lines)
  - `lower/decl.rs` (354 lines)
  - `lower/expr.rs` (353 lines)
  - `lower/stmt.rs` (261 lines)
  - `lower/helpers.rs` (97 lines)
- ✅ **`eval_stmt.rs` split into submodules**:
  - `eval_stmt/mod.rs` (eval_statement and helper functions)
  - `eval_stmt/loops.rs` (for...of, for...in, for loops)
- ✅ **Builtins already split into subdirectories**:
  - `builtins/mod.rs` - main exports
  - `builtins/array.rs` - Array constructor
  - `builtins/array_methods.rs` - Array prototype methods
  - `builtins/console.rs` - console object
  - `builtins/date.rs` - Date object
  - `builtins/error.rs` - Error constructors
  - `builtins/function.rs` - Function constructor
  - `builtins/global.rs` - globalThis and globals
  - `builtins/json.rs` - JSON.stringify/parse
  - `builtins/map.rs` - Map object
  - `builtins/math.rs` - Math object
  - `builtins/object.rs` - Object constructors
  - `builtins/promise.rs` - Promise implementation

## Architecture Summary

The runtime is now organized as:

```
crates/quench-runtime/src/
├── lib.rs              # re-exports
├── ast.rs              # AST types
├── env.rs              # environment/scopes
├── host.rs             # host function trait
├── swc_parse.rs        # swc parser wrapper
├── value/              # Value model
│   ├── mod.rs
│   ├── convert.rs
│   ├── error.rs
│   ├── function.rs
│   └── json.rs
├── builtins/           # Standard library
│   ├── mod.rs
│   ├── array.rs
│   ├── array_methods.rs
│   ├── console.rs
│   ├── date.rs
│   ├── error.rs
│   ├── function.rs
│   ├── global.rs
│   ├── json.rs
│   ├── map.rs
│   ├── math.rs
│   ├── object.rs
│   └── promise.rs
├── interpreter/        # Evaluation engine
│   ├── mod.rs
│   ├── binary_ops.rs
│   ├── call.rs
│   ├── eval_expr/
│   │   ├── mod.rs
│   │   ├── main.rs
│   │   └── helpers.rs
│   └── eval_stmt/
│       ├── mod.rs
│       └── loops.rs
└── lower/              # swc AST → runtime AST
    ├── mod.rs
    ├── decl.rs
    ├── expr.rs
    ├── helpers.rs
    ├── patterns.rs     # Pattern expansion for destructuring
    └── stmt.rs
```

## Known Limitations

1. **No file exceeds 500 lines** (linter limit).
2. **No function exceeds 40 lines** (all runtime lint warnings fixed).
3. **`builtins/json.rs`** has complexity 13 due to the required match statement for Value serialization (non-blocking warning in main crate).

## Acceptance criteria

- ✅ `cargo check -p quench-runtime` passes.
- ✅ `cargo test -p quench-runtime` passes.
- ✅ A deeply nested expression (e.g., 2000 nested binary operations) returns a clear stack-overflow error instead of a Rust panic.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```

## Future Architecture Improvements (not required)

1. **Iterative interpreter** - Replace recursive interpreter with explicit evaluation stack to handle deeply nested JSX without stack overflow.
2. **Ordered map** - Replace `HashMap` with `IndexMap` if object property enumeration order becomes observable.
3. **Garbage collection** - Add a GC to handle reference cycles that `Rc<RefCell<...>>` cannot collect.
4. **Further file splitting** - Split remaining large files like `helpers.rs` (472 lines) and `array_methods.rs` (498 lines).
