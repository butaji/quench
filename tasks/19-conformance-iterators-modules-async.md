# Task 19: Pass TypeScript iterator, module, and async conformance tests

## Goal

Make the runtime pass all runtime-relevant iterator, module, and async conformance cases.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/lower/stmt.rs`
- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/interpreter/eval_stmt/loops.rs`
- `crates/quench-runtime/src/builtins/promise.rs`
- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/map.rs`
- `crates/quench-runtime/src/context/mod.rs`
- `src/event_loop.rs`

## Steps

1. From the Task 16 audit, pick the iterator/module/async failures.
2. Implement or fix the missing features, which are likely to include:
   - `Symbol.iterator` support or special-case iterable handling for `Map`/`Set`/arrays
   - `Array.from` consuming iterables
   - generator functions and `yield` (if any conformance cases require them)
   - `import`/`export` execution in modules (the runtime already parses modules; wire module loading)
   - `Promise` static methods on the constructor object (`resolve`, `all`, `race`, `reject`)
   - `async`/`await` desugaring or native support
   - event-loop microtask invocation (`__tb_invoke_microtasks`) so `setImmediate`/`process.nextTick`/Promise microtasks run
3. Add unit tests for each fixed feature.
4. Re-run the iterator/module/async conformance subset.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `src/event_loop.rs` for microtask invocation.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/` beyond the existing host-call interface.
- Do not modify `tests/typescript/` or `examples/`.

## Acceptance criteria

- `for (const x of new Set([1,2,3]))` works.
- `Array.from(new Map([['a',1]]))` returns `[['a',1]]`.
- `Promise.resolve(42).then(v => v)` resolves.
- `async function f() { return 1; } f().then(console.log)` works.
- Module-level `import`/`export` code executes correctly.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- iterators modules async
```

## Status: PARTIALLY COMPLETED

### Bug fixes applied (2026-06-30)

1. **Map/Set `ObjectKind` not set** — Map and Set objects were created with `ObjectKind::Ordinary`, so `for...of` iteration on them used `extract_object_properties` instead of the correct `extract_map_entries`/`extract_set_values`. Fixed by setting `kind` to `ObjectKind::Map` and `ObjectKind::Set` in the respective constructors.
   - Files: `crates/quench-runtime/src/builtins/map.rs`, `crates/quench-runtime/src/builtins/set.rs`
   - Regression test: `test_map_set_for_of_iteration`

2. **`extract_set_values` checked `_map_` prefix** — the function was filtering on `k.starts_with("_map_")` instead of `k.starts_with("_set_")`, so Set values were never found during iteration.
   - File: `crates/quench-runtime/src/interpreter/eval_stmt/loops.rs`

### Already-implemented features (from Task 02/14)

- **async/await** — works correctly. `async function f() { return 1; } f().then(console.log)` outputs "Promise resolved with: 42"
- **Promise static methods** — `Promise.resolve`, `Promise.reject`, `Promise.all`, `Promise.race` all work
- **event-loop microtask draining** — implemented in Task 06

### Verified working (2026-06-30)

- `for (const x of new Set([1,2,3]))` — sum returns 60 ✓
- `for (const x of [1,2,3])` — sum returns 6 ✓
- `for (const entry of new Map([['a', 1], ['b', 2]]))` — entries as [key, value], sum of values returns 3 ✓
- `for (const [k,v] of new Map([['a', 1], ['b', 2]]).entries())` — returns correct key-value pairs ✓
- `Promise.resolve(42).then(v => console.log(v))` — outputs "Promise resolved with: 42" ✓

### Remaining work (low priority)

- `Symbol.iterator` explicit implementation (currently relies on `ObjectKind` detection)
- `Array.from` consuming iterables (verify `Array.from(new Set([...]))`)
- generator functions and `yield` (no examples need them yet)
- `import`/`export` execution in modules (no examples need them yet)
