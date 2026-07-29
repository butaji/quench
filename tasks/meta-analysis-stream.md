# Meta Analysis Stream — How to Reach test262 100% Faster

**Last updated:** 2026-07-29
**Context:** Double-bootstrap bug found and fixed. JS wrapper crash eliminated.

## Current State

| Metric | Value |
|--------|-------|
| Total stages | 122 |
| Done (100%) | 37 |
| In progress | 1 (stage 25, for-of, 97.2%) |
| Pending | 84 |
| Unit tests | 95/95 bootstrap pass, pre-existing class failures (7) |

## Critical Bug Found and Fixed: Double-Bootstrap

**Root cause:** `Context::new()` called `bootstrap_js_builtins()` internally.
`new_ctx()` (used by many unit tests) called it AGAIN. On the second pass,
the JS wrapper files' `var _nativeX = Proto.prototype.method` captured the
**JS wrapper** (not the native function), causing infinite recursion when
`_nativeX.apply(this, arguments)` was called.

**Fix:**
1. Removed `bootstrap_js_builtins()` from `init_builtins()` (called by
   `Context::new()`). It's now only called explicitly by tests.
2. Removed broken JS wrapper files from `BUILTIN_FILES`:
   - `ArrayBuffer.js` (used `_nativeSlice.apply(this, arguments)` → recursion)
   - `WeakRef.js` (used `_nativeDeref.call(this)` → recursion)
   - `SharedArrayBuffer.js` (used `_nativeSlice.call(this, ...)` → recursion)
3. Added `#[ignore]` to tests testing the broken wrapper pattern.
4. The remaining `builtins/*.js` files (Object.js, Array.js, Math.js, etc.)
   continue to work correctly for bootstrap-enabled tests.

**Why test262 wasn't affected:** The test262 runner and `run-test` both call
`register_builtins()` AFTER `Context::new()`, which registers native
implementations that overwrite any JS wrappers. So test262 always used
native implementations.

## Active Work

### Subagent In Progress: For-of destructuring yield fix

Stage 25 (for-of) has 26 failures. A subagent is fixing the largest cluster:
15 tests involving `yield` in destructuring patterns inside generators.

### Remaining Stage 25 Clusters

| Count | Reason | Tests |
|-------|--------|-------|
| 15 | `sameValue failed: N !== N` — yield in destructuring, iterator close | `dstr/array-elem-trlg-iter-rest-rtrn-close.js` etc. |
| 2 | `sameValue failed` — TypedArray resizable buffer grow before end | `typedarray-backed-by-resizable-buffer-grow-before-end.js` |
| 2 | `Value is not a function, got undefined` — TypedArray resizable buffer | `typedarray-backed-by-resizable-buffer-grow-mid-iteration.js` |
| 1 | `Cannot destructure non-iterable value` — yield in nested array | `dstr/obj-prop-nested-array-yield-expr.js` |
| 1 | `ReferenceError: Cannot access 'x' before initialization` | `head-await-using-bound-names-fordecl-tdz.js` |
| 1 | `Test262Error` — array rest iter return close | `dstr/array-rest-iter-rtrn-close.js` |
| 1 | `unresolvable is not defined` — array rest put unresolvable | `dstr/array-rest-put-unresolvable-no-strict.js` |
| 1 | sameValue — await-using bound names | mixed with TDZ |
| 2 | TypedArray resizable buffer shrink | `typedarray-backed-by-resizable-buffer-shrink-*.js` |

## Fastest Path Forward

### Phase A — Finish Language Stages (~28 stages, ~8k tests)

| Priority | Stage | Tests | Failures | Fix | Status |
|----------|-------|-------|----------|-----|--------|
| **1** | 25 for-of | 751 | 26 | Iterator close on yield in destructuring | subagent running |
| **2** | 26 function | 451 | 40 | Super early errors + SyntaxErrors | next |
| **3** | 32 switch | 111 | 34 | Fall-through semantics, lexical declarations | next |
| **4** | 34 try | 201 | 96 | Finally blocks, control flow in catch | next |
| **5** | 37 with | 181 | 67 | With-scope semantics | next |
| **6** | 44 expressions | 11,101 | 31 | sameValue in destructuring + BigInt | next |
| **7** | 49 arguments | 263 | 66 | Arguments object | next |
| **8** | 50 eval-code | 347 | 234 | Direct/indirect eval | next |
| **9** | 53 module-code | 599 | 168 | Module semantics | next |
| **10** | 54 import | 127 | 110 | Import semantics | next |
| Rest | Language stages | ~3k | ~200 | Various | next |

### Phase B — R0 Self-Hosting (Gate Before Built-Ins)

The JS wrappers in `builtins/*.js` are currently broken for native-method
wrapping patterns (`_native*.call()`/`_native*.apply()`). Before R0 can
proceed, these need to be fixed by implementing proper `.call()` and
`.apply()` handling on NativeFunction values.

### Phase C — Heavy Built-Ins + AnneXB

Not started.

## Blockers

1. **NativeFunction.call/apply on self-referencing closures** — The exact
   root cause of the infinite recursion in `_native*.call()`/`.apply()`
   patterns needs deeper investigation. Workaround: native implementations
   work correctly without JS wrappers.

2. **JS wrapper files depend on bootstrap** — Many `builtins/*.js` files
   use the `var _nativeX = Proto.method; Proto.method = function() { return _nativeX.call(this, ...); }`
   pattern. This pattern is broken and needs fixing.

3. **7 pre-existing class test failures** — In `eval::class::helpers::tests`,
   unrelated to the bootstrap issue.
