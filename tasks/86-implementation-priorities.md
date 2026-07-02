> **Ranked low-effort / high-impact decisions to move the project forward right now.**

# Task 86: Implementation priorities — low effort / high impact

## Context

We have:

- A working interpreter that passes examples and many unit tests.
- Two conformance harnesses that report pass rates and failure buckets.
- A pending performance roadmap (Task 11), a pending trampoline interpreter (Task 85), and an in-progress missing-operators task (Task 81).
- Optional chaining tests currently failing.

The goal is to pick the cheapest changes that move the needle the most toward a stable, conformant runtime.

## Ranked findings

### 1. Finish the missing operators (Task 81) — high impact, medium effort

**Why first:** The latest TypeScript expressions run shows `ReferenceError: a is not defined`, `is not a function`, and optional-chain failures as top buckets. Many of these trace back to incomplete `??`, `?.`, unary `+`, `delete`, and `||=`/`&&=`/`??=` support.

**Expected payoff:** Removing 100–200 conformance failures and unblocking optional chaining in real apps.

**Effort:** Medium. The AST variants exist; the work is wiring them through `lower/` and `interpreter/` with regression tests.

### 2. Skip "No baseline found" cases cleanly — high impact, trivial effort

**Why:** 37 TypeScript expression failures are `No baseline found`. They are harness artifacts, not runtime bugs, and they drown out actionable failures.

**Decision:** Treat a missing baseline as a `Skip` with reason `"no baseline"` rather than a `Fail`. This immediately raises the reported pass rate and makes the remaining failures real runtime bugs.

**Effort:** Trivial — change one outcome in `run_baseline_isolated_inner`.

### 3. Make `CURRENT_DEPTH` thread-local — medium impact, trivial effort

**Why:** The TypeScript harness runs each case in its own thread, but `CURRENT_DEPTH` is a global atomic. A panic or concurrent reset can corrupt the counter and cause false stack-overflow failures.

**Decision:** Convert it to `thread_local!` (like `CONTROL_FLOW` already is).

**Effort:** Trivial — one line in `interpreter/control.rs` plus the reset sites.

### 4. Add a `MAX_JS_STACK` guard to the recursive interpreter — high impact, low effort

**Why:** The recursive interpreter crashes the native Rust thread on deep recursion. Until the trampoline (Task 85) is implemented, a checked depth guard turns native stack overflow into a catchable JS `RangeError`.

**Decision:** Check `CURRENT_DEPTH` before every recursive call and return `JsError("RangeError: Maximum call stack size exceeded")` when it exceeds a limit (e.g., 10,000).

**Effort:** Low — instrument the interpreter entry points.

**Payoff:** Harnesses can run much larger subsets without process crashes.

### 5. Group `ReferenceError: X is not defined` by root cause — medium impact, low effort

**Why:** The top failure signature is `ReferenceError: A is not defined` (37 cases). It likely has a single root cause: class/namespace declarations in baselines are not hoisted or evaluated correctly.

**Decision:** Pick one example (e.g., `voidOperatorWithBooleanType.ts`), write a regression test, and fix class/namespace declaration handling in the lowerer/interpreter.

**Effort:** Low-to-medium once the root cause is confirmed.

**Payoff:** A one-line fix could remove dozens of failures.

### 6. Load test262 include files from the submodule — medium impact, medium effort

**Why:** The test262 harness stubs `assert.sameValue`, `$DONE`, etc. Real tests often rely on additional harness files (`assert.js`, `sta.js`, `compareArray.js`, `propertyHelper.js`). Missing helpers cause false failures.

**Decision:** Read includes from `tests/test262/harness/` instead of hard-coding stubs.

**Effort:** Medium — need to inject the files into each fresh context and handle frontmatter in helpers.

**Payoff:** test262 results become trustworthy and many currently-failing tests pass.

### 7. Defer the trampoline interpreter (Task 85) — very high impact, high effort

**Why:** It eliminates stack overflow by construction and enables whole-suite runs.

**Decision:** Keep it as the next large architectural change after the low-hanging fruit above. Do not start it while cheap conformance wins remain.

**Effort:** High — rewrites the interpreter loop, call dispatch, and exception handling.

### 8. Defer NaN-boxing and shapes (Task 11) — high impact, high effort

**Why:** They are performance optimizations, not correctness blockers. The runtime is already fast enough for examples and small conformance subsets.

**Decision:** Wait until correctness (conformance pass rate) and stability (stack overflow) are solved.

## Recommended order

1. Task 81 — missing operators (biggest correctness win).
2. Skip "No baseline found" (cleans up reports immediately).
3. Thread-local `CURRENT_DEPTH` + `MAX_JS_STACK` guard (stability win).
4. Fix the `ReferenceError: A is not defined` root cause (one-to-many fix).
5. Load test262 includes (trustworthy test262 numbers).
6. Task 85 — trampoline interpreter (large architecture fix).
7. Task 11 — value-model and shape optimizations (performance).

## Status

`pending` — decision document. Each item should spawn its own task when implementation starts.
