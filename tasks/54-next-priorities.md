# Task 54: Next priorities after test suite is green

## Goal

After closing all known runtime test failures, identify the next set of priorities for the quench-runtime based on the remaining review findings and conformance gaps.

## Remaining Rank 1/2 issues from Task 52 (third review)

These are the highest-impact blockers still outstanding:

### Rank 1 — Examples/end-to-end

1. **Reactive HIR nodes are decorative** (Task 22/24)
   - `Signal`/`Memo`/`Effect`/`Render` nodes exist in the AST but are never produced by the lowerer and cause errors in the interpreter.
   - Either implement them properly or remove them until they are real.
   - Status: **Pending** — tracked in Task 22/24

2. **ES module import/export is silently dropped** (Rank 1)
   - `ImportDecl`, `ExportNamedDecl`, `ExportDefaultDecl` fall through to `None` in lowering.
   - Task 19 tracks this: "Pass TypeScript iterator, module, and async conformance tests"
   - Status: **Pending** — tracked in Task 19

3. **Promise reactions are broken** (Rank 1)
   - `Promise.then`/`catch` handlers registered before resolution are silently dropped.
   - `Promise.all`/`race`/`finally` are stubs.
   - Status: **Pending**

4. **Microtasks not drained by event loop** (Rank 1)
   - `process.nextTick`, `setImmediate`, and Promise reactions are enqueued but never drained.
   - Status: **Pending**

5. **Hot reload creates and discards a new context** (Rank 1)
   - `handle_hot_reload` builds a new context but throws it away.
   - Status: **Pending**

6. **`Math.random()` returns values in ~[0, 0.233)** (Rank 1)
   - Uses nanoseconds / `u32::MAX` instead of proper random.
   - Status: **Pending** — was supposedly fixed in Task 26 but needs verification

7. **Compiler SHIMS overwrite `process` polyfill** (Rank 1)
   - `globalThis.process` is redefined unconditionally; `process.stdout.write` calls non-existent function.
   - Status: **Pending**

### Rank 2 — Language/runtime correctness

8. **`in` operator has swapped operands** (Rank 2)
   - `'x' in obj` stringifies the object instead of checking if `x` is a key.
   - Status: **Pending**

9. **`instanceof` walk starts at the wrong node** (Rank 2)
   - Should start at `left.[[Prototype]]`, not `left` itself.
   - Status: **Pending**

10. **`switch` fallthrough is lost** (Rank 2)
    - Lowered to nested if-else; cases cannot fall through.
    - Status: **Pending**

11. **Function-expression parameters lose destructuring/defaults/rest** (Rank 2)
    - Non-identifier params become placeholder `"arg"`.
    - Status: **Pending**

12. **Object-pattern rest and defaults are wrong** (Rank 2)
    - Object rest returns empty; array rest assigns single element.
    - Status: **Pending**

13. **`with` and `do...while` are silently dropped** (Rank 2)
    - `do...while` was fixed in Task 53. `with` still pending.
    - Status: **`with` pending**

14. **`delete` and unary `+` are rejected** (Rank 2)
    - Both cause `LowerError`.
    - Status: **Pending**

15. **Class static members stored on instance prototype** (Rank 2)
    - Static methods stored as `__static:<name>` on the wrong object.
    - Status: **Pending**

16. **`globalThis` disconnected from real globals** (Rank 2)
    - `globalThis.Array` etc. are undefined even though bare `Array` resolves.
    - Status: **Pending**

17. **Getter `this` binding uses prototype object** (Rank 2)
    - Walked prototype getters receive the prototype object, not the original receiver.
    - Status: **Pending**

18. **Native constructor prototypes not chained to `Object.prototype`** (Rank 2)
    - String/Number/Boolean/Date/Function/Error prototypes have no `[[Prototype]]`.
    - Status: **Pending**

### Deferred (Rank 3/4 — Architecture)

- No module HIR (`Program` only has `Script` variant)
- HIR is not A-normal form
- swc-specific types leak through public lowering API
- Public API surface exposes too many internals
- No dedicated `Pattern` type
- Source spans missing from most expressions
- No garbage collector (cycle risk)
- Recursive interpreter stack overflow risk

## Recommended priorities for next session

Based on the 80/20 rule and example impact:

1. **Verify `Math.random()` fix** — quick check that Task 26 fix works
2. **`in` operator** — easy fix, used frequently
3. **`instanceof` walk** — easy fix, used in many patterns
4. **`with` statement** — easy to add (only 2 lines in lowering)
5. **`delete` and unary `+`** — easy to add
6. **ES module import/export** — needed for TSX files with ink imports
7. **Promise reactions** — needed for async examples
8. **Microtask draining** — needed for Promise to work properly

## Verification

```bash
cargo test -p quench-runtime --test runtime_tests
cargo run --release -- examples/simple.js
cargo run --release -- examples/counter.js
cargo run --release -- examples/use-bridge.tsx --prop theme=dark
cargo run --release -- examples/animations.tsx
```
