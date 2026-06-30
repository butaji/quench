# Task 21: Strip TypeScript syntax in the lowerer

## Goal

Make the runtime parse and execute `.ts` and `.tsx` source natively by stripping or translating TypeScript-only constructs during lowering. No separate compile step.

## Status: COMPLETED (core features)

### What works:

1. **Type annotations stripped** ✓
   - `let x: number = 1;` → `let x = 1;`
   - Function parameter types stripped
   - Return type annotations stripped
   - Class member type annotations stripped

2. **Interface declarations skipped** ✓
   - `interface Foo {}` → no runtime effect

3. **Type alias declarations skipped** ✓
   - `type Bar = number;` → no runtime effect

4. **Enum declarations lowered** ✓
   - `enum Color { Red, Green }` → runtime object with forward/reverse mappings
   - `Color.Red` → `0`
   - `Color[0]` → `"Red"`

5. **Declare statements handled** ✓
   - `declare class Animal {}` → creates minimal class for `extends` compatibility
   - `declare var x: number;` → creates undefined variable declaration
   - `declare function foo(): void;` → skipped (function defined elsewhere)

6. **Type assertions stripped** ✓
   - `TsAsExpr`, `TsNonNullExpr`, `TsTypeAssertion`, `TsConstAssertion`, `TsSatisfies` → inner expression
   - `TsInstantiation` → stripped

### What still needs work:

1. **Ambient declarations in conformance baselines**
   - Tests that use JS baselines without TypeScript declarations fail
   - Example: `overrideInterfaceProperty.ts` baseline has no `declare var Mup`
   - Fix: Parse TypeScript source directly (Task 33) instead of relying on JS baselines

2. **TsParameterProperty** (constructor parameter properties)
   - `constructor(public x: number)` not yet lowered to `this.x = x`
   - Deferred: low priority for Ink examples

3. **async/await** (see Task 19)
   - `async function f() { return 1; }` needs Promise wrapping
   - `await` needs async executor integration

4. **Computed property names with await/yield**
   - `class C { [await x] = x }` not yet supported
   - Requires async support first

## TDD & testing note

- All acceptance criteria tests pass:
  - `ctx.eval_ts("let x: number = 1; x")` → `1` ✓
  - `ctx.eval_ts("interface Foo {} type Bar = number; let y: Bar = 2; y")` → `2` ✓
  - `ctx.eval_ts("enum Color { Red, Green }; Color.Red")` → `0` ✓
  - `ctx.eval_ts("function add(a: number, b: number) { return a + b; }; add(1,2)")` → `3` ✓

## Files

- `crates/quench-runtime/src/swc_parse.rs` — already uses TypeScript syntax
- `crates/quench-runtime/src/lower/expr.rs` — type assertion stripping ✓
- `crates/quench-runtime/src/lower/stmt.rs` — module declaration handling ✓
- `crates/quench-runtime/src/lower/decl_var.rs` — declare/class handling ✓

## Boundaries

- Only modified `crates/quench-runtime/src/`.
- Do not introduce a separate compile step or invoke `tsc`.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime
```
