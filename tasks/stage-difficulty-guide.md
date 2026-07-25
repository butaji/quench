# Stage Difficulty Guide

**Goal:** 100% of test262 (42,892 tests), staged, minimum LOC.
**Data source:** `tasks/index.json` (2026-07-23 full digest: 27,323/42,892 = 63.7%).

This guide ranks stages by implementation difficulty, estimates LOC per stage
(based on `index.json` columns), maps each to the crates/methods that unlock
it, and identifies single-leverage fixes that unblock multiple stages.

## Summary: Work Remaining

| Category | Tests | Failed | LOC Estimate (Rust+JS) |
|---|---|---|---|
| Language stages | 23,711 | 2,439 | 8,000–16,000 Rust |
| Built-ins stages | 23,668 | 18,817 | 2,000–4,000 Rust + 16,000–30,000 JS |
| Annex B | 1,086 | 1,086 | 300–600 JS |
| **Total remaining** | **42,892** | **15,569** | **~28,000–42,000 (Rust+JS combined)** |

Reference benchmarks: Boa ~25k Rust → 94.12% [Boa blog v0.21]; Kiesel
~50k Zig → 94.2%; QuickJS ~80k C → 83.4%.

---

## Top 10 Hardest Stages

Difficulty 7–9. Each requires significant work or external dependencies.
These are the final-stages gate.

### Stage 120 — `Temporal` (difficulty 9, 4,603 tests, 0.0%)

**Most tests of any single stage.** ES Temporal API (Stage 4, 2026-03-11):
`ZonedDateTime`, `Instant`, `PlainDateTime`, `PlainDate`, `PlainTime`,
`Duration`, `Calendar`, `TimeZone`. Calendar/timezone math via ICU4X.

- **Approach:** `temporal_rs` (Boa/Kiesel/V8-backed, ES Stage 4 stable) +
  `zoneinfo_rs` for IANA timezone data.
- **Rust LOC:** 500–1,500 (Rust-side wrapper + date math)
- **JS LOC:** 200–600 (JS facade over the crate)
- **Risk:** ICU4X API surface vs. ES spec version; evaluate before committing.
- **Crate:** `temporal_rs` + `zoneinfo_rs` (new rows in `DEPENDENCIES.md`).
- **Note:** Boa achieves 94.12% with this exact approach.

### Stage 118 — `ShadowRealm` (difficulty 9, 64 tests, 0.0%)

Isolated JavaScript global per spec. A ShadowRealm has its own global object,
`eval`, `Function`, and indirected imports. NOT a WASM sandbox — pure JS.

- **Approach:** Rust-level per-realm global + eval routing. Each ShadowRealm
  gets its own `Context` with a fresh `Realm`.
- **Rust LOC:** 500–1,000
- **Risk:** Moderate — the per-realm routing is novel; no heavy external deps.
- **Note:** WASM sandboxing (`wasmtime`) is wrong scope. ShadowRealm is
  spec-defined JS-level isolation.

### Stage 115 — `Proxy` (difficulty 8, 311 tests, 14.8%)

11 internal methods (`getPrototypeOf`, `setPrototypeOf`, `isExtensible`,
`preventExtensions`, `getOwnPropertyDescriptor`, `deleteProperty`,
`defineOwnProperty`, `hasProperty`, `get`, `set`, `apply`, `construct`) plus
invariant enforcements. Traps in Rust; invariant checks in JS.

- **Approach:** Rust traps (fast-path) + JS invariant layer (`%ProxyPrototype%`).
- **Rust LOC:** 500–1,000 (trap dispatch + `makeInvalidPropertyLabel`)
- **JS LOC:** 300–600 (invariant enforcement per spec §10.5)
- **Key challenge:** ValidateAndApplyPropertyDescriptor invariants (Sec. 10.1.6.3)
  must throw if any invariant is violated — test262 checks all 11 cases.

### Stage 44 — `expressions` (difficulty 8, 11,101 tests, 88.0%)

Largest single-stage failure count (1,336 failed). Encompasses all expression
types: binary/unary operators, literals, primary expressions, calls,
`new`, member access, `this`, `super`, comprehensions.

- **Approach:** R17 (`oxc_semantic` early errors) + per-operator eval node fixes.
  Hand-rolling early errors in `lower/` is thousands of LOC; `oxc_semantic`
  implements them already.
- **Rust LOC:** 800–2,000 (eval nodes for missing expression types)
- **JS LOC:** 0–0 (expression evaluation is Rust eval)
- **Key challenge:** `super` property access, computed property names,
  `new.target`, early errors (duplicate `<-` in expressions).

### Stage 84 — `RegExp` (difficulty 7, 1,879 tests, 42.2%)

`regress` (ES2018) does NOT support Unicode property escapes `\p{}`. Stage 84
tests `\p{Script}`, `\p{Emoji}`, `\p{General_Category}`, etc.

- **Approach:** `regex` crate with `unicode-perl` feature alongside `regress`;
  or replace `regress` if `regex` covers ES2024. Evaluate ES2018 backreferences,
  lookbehind, and dotAll too.
- **Rust LOC:** 200–400 (regex dispatch layer)
- **JS LOC:** 200–400 (RegExp built-in)
- **Risk:** `regex` crate with `unicode-perl` must cover ALL ES2024 syntax.
  `DEPENDENCIES.md` row in the same diff as the stage.
- **R18 reference:** `tasks/refactor-plan.md` §R18.

### Stage 38 — `async-generator` (difficulty 7, 301 tests, 97.0%)

Async generators: `async function* f() {}`. Combines async function
lifecycle with generator state machine. Job queue integration.

- **Approach:** S4 async→generator transform. Hand-rolling (~500 LOC) is the
  fallback; Boa achieves 94.12% without a heavy transform dependency.
- **Rust LOC:** 500–1,000 (eval nodes for async generator state)
- **JS LOC:** 0–0
- **Unlock:** Also unblocks stage 97 (`AsyncGeneratorFunction`) and stage 98
  (`AsyncGeneratorPrototype`).

### Stage 40 — `for-await-of` (difficulty 7, 1,234 tests, 89.6%)

Async iteration: `for await (x of asyncIterable)`. Requires async iterator
protocol (R2), job queue, and async→generator (S4).

- **Approach:** R2 iterator protocol + S4 async→generator + async job queue.
- **Rust LOC:** 400–800
- **JS LOC:** 0–0
- **Unlock:** Also unblocks stage 39 (`await-using`) and stage 113 (`Promise`).

### Stage 97 — `AsyncGeneratorFunction` (difficulty 7, 23 tests, 43.5%)

Same underlying mechanism as stage 38 (`async-generator`). Trivial once S4
is done.

- **Rust LOC:** 200–500 (unlocked by S4)
- **JS LOC:** 0–0

### Stage 98 — `AsyncGeneratorPrototype` (difficulty 7, 48 tests, 58.3%)

Same underlying mechanism as stage 38. Prototype methods over the async
generator state machine.

- **Rust LOC:** 300–600 (unlocked by S4)
- **JS LOC:** 0–0

### Stage 16 — `class` (difficulty 7, 4,367 tests, done ✓)

Completed 2026-07-22. R5 object model correctness + R4 dead code deletion
unblocked it. Reference implementation for future language stages.

---

## Top 10 Easiest Remaining Stages

Near-100% already. A handful of fixes each.

### Stage 13 — `async-function` (difficulty 3, 74 tests, 98.6%, **1 failed**)

Trivial: just 1 failing test stands between this stage and 100%.

### Stage 56 — `asi` (difficulty 3, 102 tests, 98.0%, **2 failed**)

Automatic Semicolon Insertion edge cases. 2 failures only.

### Stage 28 — `if` (difficulty 2, 69 tests, 95.7%, **3 failed**)

3 failures in the `if` statement eval — likely a missing early error or
a corner case in condition evaluation.

### Stage 43 — `block-scope` (difficulty 3, 145 tests, 94.5%, **8 failed**)

Block-scoped `let`/`const` in for-loop initializer, catch parameter, etc.
8 failures likely share a root cause.

### Stage 20 — `do-while` (difficulty 2, 36 tests, 80.6%, **7 failed**)

Currently in progress (stage 20). 29/36 passing.

### Stage 34 — `try` (difficulty 3, 201 tests, 44.3%, **112 failed**)

112 failures — but likely one root cause (missing `catch` binding or
`finally` evaluation). Fix once, get 112 tests.

### Stage 21-33, 35-37 — `empty`, `return`, `throw`, `break`, `continue`, etc.

Done or near-done. These stages are Rust eval nodes only.

---

## Optimal Implementation Order (Phase 1–5)

### Phase 1 — Clear easy wins (1–2 weeks, ~500 LOC)

Land the near-100% stages first for quick progress signals:
1. Stage 13 `async-function` — 1 test fix
2. Stage 56 `asi` — 2 test fix
3. Stage 28 `if` — 3 test fix
4. Stage 43 `block-scope` — 8 test fix
5. Stage 20 `do-while` — 7 test fix
6. Stage 34 `try` — 112 test fix (one root cause)

**LOC: ~500 Rust. Tests unlocked: ~150.**

### Phase 2 — Language stages via R17 `oxc_semantic` (2–4 weeks)

R17 unlocks the bulk of remaining language failures:
- Stage 44 `expressions` (1,336 failed) — early errors via `oxc_semantic`
- Stage 50 `eval-code` (234 failed) — same early error path
- Stage 51 `global-code` (21 failed)
- Stage 53 `module-code` (168 failed)
- Stage 54 `import` (110 failed)
- Stage 25 `for-of` (266 failed) — R2 iterator protocol

**LOC: ~1,000 Rust. Tests unlocked: ~2,500.**

### Phase 3 — R0 self-hosting starts (4–8 weeks)

Move built-ins to JS. Each builtin deleted from Rust saves LOC and
simplifies future fixes. R0 order:
1. `Object` → `Function` → `Error` → `Symbol` → `Number` → `Boolean`
2. `String` → `Array` → `Iterator` → `Map`/`Set`/`Weak*`
3. `Promise` → `JSON` → `Reflect`/`Proxy` → `Math`
4. `RegExp` (shell over `core/regex.rs`) → `Date` (shell over `core/date.rs`)
5. `BigInt` → `TypedArray`/`ArrayBuffer`/`DataView`/`Atomics`

**Rust LOC saved: ~3,000. JS LOC added: ~10,000. Tests unlocked: ~10,000.**

### Phase 4 — Async/Promise (2–4 weeks)

- S4 async→generator (swc or hand-rolled)
- R2 one iterator protocol
- `Promise` stage (729 tests)
- `for-await-of` (1,234 tests)
- `async-generator` (301 tests)
- `AsyncFunction`/`AsyncGenerator*` (89 tests)

**LOC: ~1,500 Rust + 500 JS. Tests unlocked: ~3,000.**

### Phase 5 — Hard built-ins + Temporal (4–8 weeks)

- `Proxy` (311 tests) — Rust traps + JS invariants
- `TypedArray`/`TypedArrayConstructors` (2,184 tests)
- `ArrayBuffer`/`SharedArrayBuffer`/`DataView` (886 tests)
- `Temporal` (4,603 tests) — `temporal_rs` + ICU4X
- `ShadowRealm` (64 tests)

**LOC: ~3,000 Rust + 2,000 JS. Tests unlocked: ~8,000.**

---

## Single-Leverage Fixes (one fix → many stages)

| Fix | Stages unlocked | Estimated LOC |
|---|---|---|
| R17 `oxc_semantic` early errors | 44, 50, 51, 53, 54, 26, 23 | ~800 Rust |
| R2 one iterator protocol | 25, 87, 88–94, 40 | ~400 Rust |
| R0 self-hosting (Object.js) | 71, 85, 82, 72, 80, 83 | saves ~3k Rust |
| S4 async→generator | 38, 40, 97, 98, 99, 39, 113 | ~500 Rust |
| R3 `chrono` Date core | 81 (Date built-in) | ~50 Rust |
| R18 RegExp Unicode `\p{}` | 84 | ~300 Rust + 300 JS |
| `temporal_rs` crate | 120 | ~1,000 Rust + 400 JS |

---

## Critical Path Summary

```
R17 (oxc_semantic) → R2 (iterator) → S4 (async→generator)
→ R0 (self-hosting Object/Array/String)
→ TypedArray/Promise/Proxy
→ Temporal (temporal_rs)
```

Every step above is a **prerequisite for 3+ later stages**, making them
higher-leverage than any stage-specific fix.

---

## macOS / Darwin Compatibility

All remaining stages are portable — no Darwin-specific code needed:
- **Atomics** (stage 106): `std::sync::atomic` works natively on macOS.
- **Date/Temporal**: `chrono`/`temporal_rs` handle timezones portably.
- **SharedArrayBuffer**: test262 skips tests when cross-origin isolation
  headers are absent — no OS-level work.
- **All file I/O** is in the harness (`tools/run-each.sh`), not the runtime.

Profiling on macOS: `cargo-flamegraph` uses `xctrace` (Apple Instruments CLI)
under the hood. See `docs/architecture.md` §Profiling.

---

## Sources

- Boa v0.21 NaN boxing: <https://boajs.dev/blog/2025/10/22/boa-release-21>
- Boa test262 94.12%: <https://boajs.dev/blog/2025/10/22/boa-release-21>
- LibJS SerenityOS: <https://test262.fyi/>
- QuickJS NG 83.4%: <https://test262.fyi/>
- Rust Performance Book: <https://nnethercote.github.io/perf-book/>
- `temporal_rs` ES Stage 4: ECMA-402 / TC39 stage records
- `DEPENDENCIES.md`: verified crate usage and alternatives
