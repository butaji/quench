> **Outcome of a full analysis of the test262 and TypeScript conformance harnesses.**

# Task 76: Analyze and enable whole-suite conformance runs

## Goal

Understand why the test262 and TypeScript conformance harnesses cannot yet run their full suites, document the blockers, and create a concrete plan to remove them.

## Current harness state

| Harness | Entry | Runner | Isolation | Typical run |
|---------|-------|--------|-----------|-------------|
| test262 | `crates/quench-runtime/tests/test262.rs` | `src/test262/runner.rs` | Fresh `Context` per file; `catch_unwind`; same thread | Tiny hard-coded subsets (~40–343 files each) |
| TypeScript | `crates/quench-runtime/tests/conformance.rs` | `src/conformance/typescript/mod.rs` | Fresh `Context` per case; **one thread per case** | `expressions/` subset (376 cases) + 100-case sanity |

**Target state for test262:** execute **every** `.js` file under `tests/test262/test` with **zero skips**. Unsupported features must produce failures, not skips.

Both harnesses register helpers as Rust native functions. No JS helper strings are injected.

## Suite sizes

- **test262:** ~53,683 `.js` files under `tests/test262/test`.
- **TypeScript conformance:** ~5,907 `.ts`/`.tsx` files under `tests/typescript/tests/cases/conformance`.

## Latest results

- **TypeScript expressions (376 cases):** 124 passed, 252 failed, 0 skipped → 33% pass rate.
- **TypeScript 100-case sanity:** 12 passed, 49 failed, 39 skipped → 12% pass rate.
- **test262:** only tiny subsets have been run recently; no recent full-subset report exists.

## Blockers to full-suite runs

### 1. Native stack exhaustion from the recursive interpreter

- The interpreter evaluates JS recursively and consumes a large amount of native Rust stack per call.
- test262 already limits itself to tiny subsets because running more than a few hundred files causes stack overflow.
- TypeScript spawns a thread per case, which contains the crash, but the harness still records `Maximum call stack size exceeded` failures.

### 2. Broken per-thread recursion tracking

- `crates/quench-runtime/src/interpreter/control.rs` uses a **global** `AtomicUsize` for `CURRENT_DEPTH`.
- The TypeScript harness spawns one thread per case and calls `reset_depth()`. Because the counter is shared across threads, depth tracking is not actually isolated.
- Fix: make `CURRENT_DEPTH` `thread_local!` (like `CONTROL_FLOW` already is).

### 3. test262 feature skip list prevents a true full-suite run

test262 currently skips 84 feature categories, including:

- `Promise`, async functions, generators
- Classes, private fields, decorators
- `BigInt`, `Symbol`, `Proxy`, `Reflect`, `WeakMap`, `WeakSet`, `WeakRef`
- `TypedArray`, all `RegExp` features
- Modules (`import`/`export`)
- Spread, destructuring, template literals, optional chaining, nullish coalescing, logical assignment
- Many newer built-ins (`Object.hasOwn`, `Array.prototype.groupBy`, `String.prototype.replaceAll`, etc.)

**This is a harness problem, not a runtime problem.** The harness must run every test and let failures be bucketed by root cause. Removing the skip list is a low-effort, high-impact change because it immediately reveals the real compatibility percentage and the largest failure buckets.

TypeScript conformance skips by policy (acceptable for now):

- `@noEmit`, `@emitDeclarationOnly`
- Non-ES module systems (`amd`, `umd`, `system`, `node16`, `nodenext`, `none`)
- React JSX runtime cases
- Directories: `types/`, `interfaces/`, `symbols/`, `namespaces/`, `decorators/`, `ambient/`, `constEnums/`, `declarationEmit/`, `additionalChecks/`
- `.d.ts` and `.tsx` files

### 4. Baseline and harness gaps

- Many TypeScript cases fail with `No baseline found` or because the baseline extractor mis-parses the emitted-JS section.
- test262 include files (`assert.js`, `sta.js`, `eq.js`, etc.) are not loaded from `tests/test262/harness/`; only a small set of helpers is stubbed in Rust.

### 5. Runtime bugs visible in the suites

Top TypeScript failure signatures:

- `ReferenceError` — missing globals or incorrect scope resolution.
- `Invalid computed property` — likely object-key coercion bug.
- `is not a function` — missing built-in methods.
- `Parse error` — parser does not yet handle some syntax the harness tries to run.
- `Maximum call stack size exceeded` — recursive interpreter overflow.

## Plan to enable whole-suite runs

### Phase 1: Stability

1. **Implement the trampoline interpreter** (Task 85). This is the canonical fix: replace recursive `eval` with a heap-allocated `Vec<CallFrame>` and a single loop, using `&mut Context` and slot-indexed object storage. Native stack overflow disappears; runaway JS recursion becomes a controllable `RangeError`.
2. Fix `CURRENT_DEPTH` to be `thread_local!` as a short-term guard until the trampoline lands.
3. Make the test262 runner spawn one thread per test file (matching TypeScript) as a belt-and-suspenders isolation measure. Use `rayon` for parallel test execution with a fresh isolate/context per test.

### Phase 2: Coverage with zero test262 skips

1. Remove the test262 feature skip list entirely; run every `.js` file under `tests/test262/test`.
2. Execute each test262 file in an isolated thread with a fresh context and a short per-test timeout.
3. Generate a complete failure report with top failure signatures and per-feature pass rates.
4. Run the full TypeScript conformance suite (policy skips remain) to generate a complete failure report.
5. Bucket failures by root cause (missing built-in, parser gap, scope bug, object-model bug, stack overflow).

### Phase 3: Fix runtime bugs with regression tests

1. Pick the highest-count failure bucket.
2. Write a focused Rust regression test in `crates/quench-runtime/tests/`.
3. Fix the smallest runtime change that makes the regression test pass.
4. Re-run the full suite and watch the bucket shrink.

### Phase 4: Expand supported features

1. As runtime support lands, observe feature buckets moving from failing to passing naturally.
2. Remove TypeScript policy skip rules only after source-direct TS evaluation can handle those cases.

## Boundaries

- Harness changes are allowed in `crates/quench-runtime/tests/` and `crates/quench-runtime/src/{test262,conformance}/`.
- Runtime fixes are allowed in `crates/quench-runtime/src/`.
- Do not modify `examples/`, `tests/test262/`, or `tests/typescript/`.

## Verification

```bash
./scripts/run_tests.sh test-test262
./scripts/run_tests.sh test-conformance
cargo test -p quench-runtime
```

A successful full-suite run produces `target/test262_report.json` and `target/conformance_report.json` without process aborts.

## Targets

- **Suite:** `both`
- **Batch:** 7
- **Target subset:** Full `tests/test262` + `tests/typescript` conformance suites
- **Blocked by:** 85, 88, 264
- **Exit criteria:** Both full conformance suites run to completion and reports are regenerated with accurate pass/fail/skip counts.
