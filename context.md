# quench-runtime Audit — Remaining Issues

## Test Suite Status
- `cargo test -p quench-runtime --test runtime_tests`: **PASSING** (92 passed; 0 failed; 1 ignored)
- `test_math_random_range` now passes (was previously failing, likely fixed in Task 26)

## Grep Findings

### 1. `Err(LowerError` in `stmt.rs`
11 instances, all intentional rejection of unsupported syntax:
- ES module exports: `export default function/class`, `export { ... }`, `export * from`, `export const/let/var`, `export function`, `export class`, `export interface`, `export type`, `export enum` (lines 71–132)
- `with` statement (line 220)

These are deliberate errors for unsupported features, not bugs.

### 2. Debug `eprintln` statements: 103 occurrences
- `eprintln.*DEBUG`: 50 occurrences
- Total `eprintln` calls: 103
- Likely debug logging that should be removed or gated behind a feature flag before release.

### 3. `unimplemented`/`todo!` in quench-runtime/src/
**None found** — the codebase is clean of placeholder code.

### 4. Reactive AST nodes in `ast.rs`
```
242: Signal {         — exists, decorative (Task 22/24)
247: SignalGet {       — exists, decorative
251: SignalSet {       — exists, decorative
255: Memo {            — exists, decorative
261: Effect {          — exists, decorative
268: Render {          — exists, decorative
```
These are real types but never produced by the lowerer.

## Rank 1 Issues (from Task 54)

| # | Issue | Status |
|---|-------|--------|
| 1 | Reactive HIR nodes decorative | Pending (Task 22/24) |
| 2 | ES module import/export silently dropped | Pending (Task 19) |
| 3 | Promise reactions broken | Pending |
| 4 | Microtasks not drained | Pending |
| 5 | Hot reload discards context | Pending |
| 6 | `Math.random()` broken | **FIXED** (tests pass) |
| 7 | Compiler SHIMS overwrite `process` | Pending |

## Rank 2 Issues

| # | Issue | Status |
|---|-------|--------|
| 8 | `in` operator swapped operands | Pending |
| 9 | `instanceof` walk wrong node | Pending |
| 10 | `switch` fallthrough lost | Pending |
| 11 | Function params lose destructuring | Pending |
| 12 | Object-pattern rest/defaults wrong | Pending |
| 13 | `with` statement dropped | Pending |
| 14 | `delete` and unary `+` rejected | Pending |
| 15 | Class static members on wrong object | Pending |
| 16 | `globalThis` disconnected | Pending |
| 17 | Getter `this` binding wrong | Pending |
| 18 | Native prototypes not chained | Pending |

## Summary

**Good news:** Tests are green, no `todo!`/`unimplemented` left, `Math.random()` is fixed.

**Remaining high-impact issues:**
1. **103 debug `eprintln` calls** — should be cleaned up for release
2. **ES module import/export** — blocks ink TSX examples with imports
3. **`with` statement** — only 2 lines needed in lowering per Task 54
4. **`in` operator** — easy fix, frequently used
5. **`instanceof` walk** — easy fix
6. **Promise reactions + microtask draining** — needed for async examples
7. **Reactive HIR nodes** — either implement or remove decorative types
