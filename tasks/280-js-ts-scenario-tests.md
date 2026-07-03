# Task 280: Add JS/TS scenario test harness

## Status: PENDING

## Goal

Run real JavaScript and TypeScript code snippets through the runtime from Rust unit tests so runtime behavior is continuously validated against actual user-facing source code.

## Why

Pure Rust unit tests are great for internals, but the runtime's real contract is executing JS/TS/JSX/TSX. Scenario tests make regressions obvious by running the same kinds of snippets users write.

## Design

- **Scenario files:** `crates/quench-runtime/tests/scenarios/<category>/<name>.js` or `.ts`.
- **Rust harness:** `crates/quench-runtime/tests/scenarios/mod.rs` provides helpers:
  - `run_js(path) -> Result<Value, JsError>`
  - `run_ts(path) -> Result<Value, JsError>`
  - `assert_eq_js(path, expected)`
  - `assert_throws_js(path, expected_error_type)`
- **One scenario per file.** Each `.js`/`.ts` file tests one behavior.
- **Categories:**
  - `expressions/` — arithmetic, comparison, logical, optional chaining
  - `statements/` — if, loop, try/catch, switch
  - `functions/` — call/apply/bind, arrow, default/rest, closures
  - `objects/` — property access, prototype, class construction
  - `arrays/` — methods, spreading, destructuring
  - `typescript/` — type erasure, `as const`, enums, namespaces
  - `jsx/` — JSX transform to Ink calls
- **Expected output** can be embedded as a comment at the top of the scenario file or kept in a paired `.stdout`/`.stderr` file.

## Example scenario

`tests/scenarios/expressions/nullish_coalescing.js`:

```js
let a = null;
let b = a ?? 42;
b;
```

Rust test:

```rust
#[test]
fn nullish_coalescing_returns_default_for_null() {
    let result = run_js("scenarios/expressions/nullish_coalescing.js").unwrap();
    assert_eq!(result, Value::Number(42.0));
}
```

## Acceptance criteria

- [ ] `tests/scenarios/` directory exists with category subfolders.
- [ ] Rust harness can run `.js` and `.ts` scenarios and assert on results/errors.
- [ ] At least one scenario exists for each currently open spec task (250, 251, 241, etc.).
- [ ] Running `cargo test -p quench-runtime scenarios` executes all scenario tests.

## Dependencies

- Task 279 (granular unit-test policy)
- Task 264 Phase 0 quick wins (many scenarios will validate those fixes)
