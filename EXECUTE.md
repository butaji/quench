# runts-ink: Execution Guide

> **Architecture:** rquickjs (dev engine) + Yoga (layout) + Ratatui (render).
> **HIR interpreter:** DELETED. Do not restore.
> **Taffy:** REMOVED. Yoga is the sole layout engine.
> **Goal:** 100% look&feel parity across 3 environments for all Ink examples, and maximum TS/TSX coverage in HIR + compile-path codegen.
> **Parity standard:** 100% output match. Zero divergence between deno, `runts dev`, and `runts build`.

---

## The 3 Environments

| # | Environment | What it is | How to invoke |
|---|-------------|-----------|---------------|
| 1 | **deno** | Reference TypeScript runtime (npm:ink) | `deno run -A main.tsx` |
| 2 | **rq** (runts dev) | TSX → JS (oxc_codegen) → rquickjs + Yoga bridge → render | `runts dev --once --plugin ratatui ./example` |
| 3 | **compile** (runts build) | TSX → HIR → Rust codegen → `cargo build --release` | `runts build --release --plugin ratatui ./example` |

**Parity means:** The rendered text output (after ANSI normalization) is **identical** across all 3 environments for every example. 100% match. No exceptions.

---

## Workflow

All work is tracked in `tasks/`. Check `tasks/index.json` for the current task breakdown, priorities, and statuses. Each task has a matching `tasks/xxx-title.md` file with acceptance criteria and implementation notes.

**The rule:** Pick one pending task, implement it fully, verify acceptance criteria, commit, push. Do not batch multiple tasks into a single commit.

### Before you start any task

1. Read `tasks/index.json` to find the next pending task.
2. Read its matching `tasks/xxx-title.md` for acceptance criteria.
3. Run `cargo build` to confirm the current state.

### After you finish a task

1. Verify acceptance criteria from the task file.
2. Update `tasks/index.json` to mark the task completed.
3. `git add -A && git commit -m "brief description"`
4. `git push origin fresh`

---

## Phases

### Phase 0: Unblock
**Goal:** `cargo build` passes. Linter is enforced.

**Tasks:** 020, 021 | **Status:** ✅ Completed

---

### Phase 1: rquickjs + Yoga Engine
**Goal:** `runts dev --once` renders any example identically to deno.

**Tasks:** 022–026 | **Status:** ✅ Completed

---

### Phase 2: Compile + Verification
**Goal:** `runts build --release` produces working binaries. One parity harness.

**Tasks:** 027–029, 034 | **Status:** ✅ Completed

---

### Phase 3: Coverage Gaps
**Goal:** No feature is untested or unexercised.

**Tasks:** 033, 035, 036 | **Status:** ✅ Completed

---

### Phase 4: Cleanup + Future
**Goal:** Repo is clean. Docs are truthful.

**Tasks:** 030–032 | **Status:** ✅ Completed

---

### Phase 5: Compile Path Hardening
**Goal:** Core codegen bugs fixed. `runts build --release` works for simple static examples.

**Tasks:** 037–040 | **Status:** ✅ Completed

---

### Phase 6: Example-Driven Feature Coverage
**Goal:** Every practical TS/TSX/React/Ink feature is exercised by at least one Ink example, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

**Strategy:** Create real Ink TUI examples that use specific features. The parity harness automatically validates those features in deno, `runts dev`, and `runts build`.

| Task | Example | Feature | Codegen / HIR Fix |
|------|---------|---------|-------------------|
| 041 | — | Enable `spec_expressions` + `spec_types` test modules | Wire test modules |
| 042 | `ink-control-flow` | `for`, `while`, `do-while`, `switch`, `break`, `continue` | Verify `gen_for`, `gen_while`, `gen_switch` |
| 043 | `ink-try-catch` | `try`, `catch`, `finally`, `throw` | Verify `gen_try`, `gen_throw` |
| 044 | `ink-forin-forof` | `for-in`, `for-of`, iterators | Verify `gen_for_in`, `gen_for_of` |
| 045 | `ink-destructure` | Destructuring, defaults, rest | Fix `Pat::Default`, `Pat::Rest` |
| 046 | `ink-spread` | Object/array spread, JSX spread | Verify `gen_object_expr`, `gen_array_expr` |
| 047 | `ink-template` | Template literals, multiline | Verify `gen_template_expr` |
| 048 | `ink-object-advanced` | Getters, setters, computed keys, methods | Implement `Get`/`Set`/`Method` in `gen_object_expr` |
| 049 | `ink-nullish-optional` | `??`, `?.` (optional chaining) | Requires Task 068 (HIR `OptionalMember`) |
| 050 | `ink-typeof-guard` | `typeof`, `instanceof`, `delete`, `void` | Add `typeof` constant folding |
| 051 | `ink-compound-bitwise` | All compound assignment + bitwise operators | Add 7 missing compound assign + bitwise arms |
| 052 | `ink-async-fetch` | `async`, `await`, Promise | Verify async closure codegen |
| 053 | `ink-generator` | `function*`, `yield`, `yield*` | Requires Task 072 (generator body parsing) |
| 054 | `ink-function-params` | Default params, rest params | Parse defaults/rest in `func_expr_params` |
| 055 | `ink-class-component` | Classes, `extends`, `super` | Implement `gen_class` (currently `None`) |
| 056 | `ink-static-private` | Static methods, private fields `#field` | Requires Task 071 (private fields in HIR) |
| 057 | `ink-getter-setter` | Getters, setters, computed accessors | Implement getter/setter codegen |
| 058 | `ink-module-exports` | Named/default/re-exports, namespace imports | Verify module codegen |
| 059 | `ink-dynamic-import` | `import()`, `import.meta` | Requires Task 073 (dynamic import in HIR) |
| 060 | `ink-react-advanced` | `useReducer`, `useContext`, `memo`, `forwardRef` | Verify React shim |
| 061 | `ink-jsx-advanced` | Spread attrs, dynamic components, fragments, conditional | Verify JSX codegen |
| 062 | `ink-animation` | `useAnimation` | Verify bridge hook |
| 063 | `ink-measure` | `measureElement`, `useBoxMetrics` | Verify bridge hook |
| 064 | `ink-focus-paste` | `useFocus`, `useFocusManager`, `usePaste` | Verify bridge hooks |
| 065 | `ink-static-transform` | `Static`, `Transform`, `Newline`, `Spacer` | Verify bridge components |
| 066 | `ink-enum-types` | Enums, `as`, `satisfies` | Requires Tasks 069–070 |
| 067 | `ink-type-erasure` | Generics, mapped types, conditional types | Type erasure (no HIR needed) |

**Rule:** If an example compiles in deno but fails in `runts build`, the codegen bug must be fixed as part of that task.

**Tasks:** 041–067 | **Status:** ✅ Completed (27 tasks)

---

### Phase 7: HIR & Parser Expansion
**Goal:** HIR can represent 100% of practical TS/TSX constructs.

Some TS features are parsed by oxc but dropped or converted to `Expr::Invalid` before reaching HIR:

| Feature | Parser | HIR | Task |
|---------|--------|-----|------|
| Optional chaining `?.` | ✅ oxc | ✅ Added | 068 |
| `as` / `satisfies` / `!` | ✅ oxc | ✅ Added | 069 |
| Enum declarations | ✅ oxc | ✅ Added | 070 |
| Private fields `#field` | ✅ oxc | ✅ Added | 071 |
| Generators `function*` | ✅ oxc | ✅ Added | 072 |
| Dynamic import `import()` | ✅ oxc | ✅ Added | 073 |
| Decorators | ✅ oxc | ⚠️ Partial | 074 |
| Type aliases | ✅ oxc | ❌ Missing | — (Phase 10, Task 082) |
| Interface declarations | ✅ oxc | ❌ Missing | — (Phase 10, Task 082) |

**Tasks:** 068–074 | **Status:** ✅ Completed except 074 (6/7 done)

---

### Phase 8: Compile-Path Integration Tests
**Goal:** The compile path is thoroughly tested with real generated code.

**Tasks:** 075–077 | **Status:** 🔄 075 Completed, 076–077 Pending (1/3 done)

---

### Phase 9: Final Audit
**Goal:** Document the exact coverage matrix. Set v1.0 targets.

**Tasks:** 078 | **Status:** 🔄 Pending (1 task)

---

### Phase 10: Extended TS/TSX Coverage
**Goal:** Every remaining practical TS/TSX/React/Ink feature is exercised by at least one Ink example, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 079 | `ink-logical-assign` | `\|\|=`, `&&=`, `??=` | P1 |
| 080 | `ink-use-layout-effect` | `useLayoutEffect` | P1 |
| 081 | `ink-use-id-transition` | `useId`, `useTransition` | P1 |
| 082 | `ink-type-alias-interface` | Type aliases, interfaces | P1 |
| 083 | `ink-access-modifiers` | `public`/`private`/`protected`/`readonly` | P1 |
| 084 | `ink-top-level-await` | Top-level `await` | P1 |
| 085 | `ink-import-export-type` | `import type`, `export type` | P1 |
| 086 | `ink-barrel-export` | `export * from`, `import * as` | P1 |
| 087 | `ink-keyof-readonly` | `keyof`, `readonly` arrays | P1 |
| 088 | `ink-bigint-globalthis` | BigInt, numeric separators, `globalThis` | P2 |
| 089 | `ink-symbol-collections` | Symbol, Map, Set, WeakMap | P2 |
| 090 | `ink-suspense-lazy` | `Suspense`, `lazy` | P2 |
| 091 | `ink-error-boundary` | `<ErrorBoundary>` | P2 |
| 092 | `ink-namespace-declare` | `namespace`, `declare` | P2 |
| 093 | `ink-override-implements` | `override`, `implements` | P2 |
| 094 | `ink-abstract-class` | `abstract` classes | P2 |
| 095 | `ink-new-target` | `new.target` | P3 |
| 096 | `ink-reflect-api` | `Reflect` API | P3 |
| 097 | `ink-template-literal-types` | Template literal types | P3 |
| 098 | `ink-infer-conditional` | `infer` in conditional types | P3 |
| 099 | `ink-regexp-advanced` | RegExp flags, `matchAll` | P3 |

**Tasks:** 079–099 | **Status:** 🔄 Pending (21 tasks)

---

### Phase 11: Type System + Runtime API Deep Coverage
**Goal:** Every remaining practical TS/TSX/React/Ink feature is exercised by at least one Ink example, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 100 | `ink-utility-types` | `Partial`, `Required`, `Pick`, `Omit`, `Record`, `ReturnType` | P1 |
| 101 | `ink-as-const` | `as const`, literal types, tuple types | P1 |
| 102 | `ink-index-intersection` | Index signatures, intersection types (`A & B`) | P1 |
| 103 | `ink-unknown-never` | `unknown`, `never`, user-defined type guards | P1 |
| 104 | `ink-array-modern` | `flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `includes`, `findLast` | P1 |
| 105 | `ink-object-modern` | `fromEntries`, `hasOwn`, `getOwnPropertyDescriptors` | P1 |
| 106 | `ink-use-imperative-handle` | `useImperativeHandle`, `forwardRef` | P1 |
| 107 | `ink-use-sync-external-store` | `useSyncExternalStore`, `useDeferredValue` | P1 |
| 108 | `ink-context-advanced` | Context `displayName`, `defaultValue`, multiple providers | P1 |
| 109 | `ink-discriminated-unions` | Discriminated unions, exhaustive checks | P1 |
| 110 | `ink-mapped-types` | Mapped types `{ [K in T]: V }` | P1 |
| 111 | `ink-proxy` | `Proxy` handler | P2 |
| 112 | `ink-weakref` | `WeakRef`, `FinalizationRegistry` | P2 |
| 113 | `ink-string-modern` | `padStart`, `padEnd`, `replaceAll`, `trimStart`, `trimEnd`, `at` | P2 |
| 114 | `ink-promise-advanced` | `allSettled`, `any`, `race`, `withResolvers` | P2 |
| 115 | `ink-this-parameter` | `this` parameter, `this` types | P2 |
| 116 | `ink-unique-symbol` | `unique symbol`, branded types | P2 |
| 117 | `ink-react-children` | `Children` API, `cloneElement`, `isValidElement` | P2 |
| 118 | `ink-date-math` | `Date`, `Math`, `Intl` | P2 |
| 119 | `ink-export-equal` | `export =`, `import = require()` | P3 |
| 120 | `ink-global-augmentation` | `declare global`, `declare module` | P3 |

**Tasks:** 100–120 | **Status:** 🔄 Pending (21 tasks)

---

## Known Coverage Gaps (Current State)

### Test Coverage
All **15/15** test modules are enabled. `spec_expressions` and `spec_types` are wired in (Task 041 completed).

**Result:** 983 passed; 0 failed; 183 ignored.

### Compile-Path Coverage: ~70%

| Layer | Coverage | Notes |
|-------|----------|-------|
| Parser (oxc) → HIR | ~80% | 38 Expr variants, 24 Stmt variants |
| HIR → Rust codegen | ~70% | Core constructs have codegen; advanced TS features pending |
| Compile-path integration tests | 53 tests | `tests/compile_codegen.rs` — covers most P0 constructs |

### Features Without Examples (43 gaps)

124 examples exist. Phase 6 examples (Tasks 042–067) cover the core TS/TSX/React/Ink features. Phase 10 examples (Tasks 079–099) cover extended features. Phase 11 examples (Tasks 100–120) cover type system deep features and modern runtime APIs. See `tasks/index.json` → `coverage_gaps.features_without_examples` for the full list.

Remaining gaps (Phase 10–11 targets):
- Logical assignment: `||=`, `&&=`, `??=`
- React hooks: `useLayoutEffect`, `useId`, `useTransition`, `useImperativeHandle`, `useSyncExternalStore`, `useDeferredValue`
- Type declarations: type aliases, interfaces, `namespace`, `declare`
- Class modifiers: `public`/`private`/`protected`/`readonly`, `abstract`, `override`, `implements`
- Module patterns: `import type`, `export type`, barrel exports (`export * from`), `export =`, `import = require()`
- Advanced JS: top-level `await`, BigInt, `globalThis`, Symbol, Map/Set, Proxy, WeakRef
- React patterns: `Suspense`/`lazy`, `ErrorBoundary`, `Children` API, `cloneElement`, `isValidElement`
- Context advanced: `displayName`, `defaultValue`, multiple providers
- Type system: `keyof`, template literal types, `infer`, utility types (`Partial`, `Required`, `Pick`, `Omit`, `Record`), `as const`, mapped types, discriminated unions, index signatures, intersection types, `unknown`, `never`, type guards, `unique symbol`, branded types, `this` parameter, global/module augmentation
- Meta-properties: `new.target`
- Reflection: `Reflect` API
- RegExp: `matchAll`, advanced flags
- Modern array methods: `flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `includes`, `findLast`
- Modern object methods: `fromEntries`, `hasOwn`, `getOwnPropertyDescriptors`
- Modern string methods: `padStart`, `padEnd`, `replaceAll`, `trimStart`, `trimEnd`, `at`
- Promise advanced: `allSettled`, `any`, `race`, `withResolvers`
- Globals: `Date`, `Math`, `Intl`

---

## Parity Harness Specification

The single script (`scripts/parity.sh`) MUST:

1. **Run each example in all 3 environments** (or subset via `--env`).
2. **Normalize output** before comparison:
   - Strip ANSI escape codes
   - Normalize trailing whitespace
   - Normalize line endings to `\n`
3. **Compare symbol-by-symbol**, not line-by-line.
4. **Report per-example:**
   ```json
   {
     "example": "ink-counter",
     "deno": { "status": "ok", "similarity": 100.0 },
     "rq": { "status": "ok", "similarity": 100.0 },
     "compile": { "status": "ok", "similarity": 100.0 }
   }
   ```
5. **Handle interactive examples:** Detect `useInput`, `useFocus`, `useStdin` in source. Capture only the **initial static frame**.
6. **Exit 0** if all similarities = 100%, else exit 1.

---

## DO NOT (Anti-patterns)

| Trap | Why |
|------|-----|
| **Do not restore or expand the HIR interpreter.** | It was 3,087 lines of a broken custom JS engine. rquickjs gives 100% JS semantics for ~1MB. |
| **Do not keep Taffy as a fallback.** | Yoga is the same engine Ink uses. Two layout engines = 2× bug surface. |
| **Do not add new shell scripts.** | Multiple scripts already exist. ONE script. Parameterize it. |
| **Do not write hook polyfills in Rust.** | Hooks run inside rquickjs. The bridge only exposes Rust primitives. |
| **Do not exceed linter limits.** | 500 lines/file, 40 lines/fn, 10 complexity. Extract, don't negotiate. |
| **Do not commit without `cargo build` passing.** | Fix first, then iterate. |
| **Do not batch multiple tasks in one commit.** | One task = one commit = one push. |
| **Do not leave test modules commented out.** | Disabled tests are invisible decay. Fix or delete. |
| **Do not add examples that require Rust code.** | Examples are pure TS/TSX only. |
| **Do not accept < 100% parity.** | The standard is identical output. Fix the bug, not the threshold. |

---

## Quick Debug Flow

```bash
# 1. Check build
cargo build

# 2. Test one example against deno
deno run -A examples/ink-text-props/main.tsx > /tmp/deno.txt
runts dev --once --plugin ratatui examples/ink-text-props > /tmp/rq.txt
diff /tmp/deno.txt /tmp/rq.txt

# 3. Test compile path
runts build --release --plugin ratatui examples/ink-text-props
examples/ink-text-props/target/release/runts-app > /tmp/compile.txt

# 4. Run parity harness
./scripts/parity.sh --env all --examples ink-text-props --verbose

# 5. Check compile-path tests
cargo test --test compile_codegen

# 6. Check linter
# (build.rs runs automatically during cargo build)
```

---

## Success Criteria (Final Checklist)

### Completed ✅
- [x] `cargo build` passes with 0 errors, 0 warnings.
- [x] `cargo test --test rq_parity` passes ≥90% of examples (120/123 active).
- [x] `cargo test --bin runts` exits 0 (983 passing, 183 ignored).
- [x] `cargo test --test compile_codegen` passes (53/53).
- [x] Zero commented-out test modules.
- [x] No file > 500 lines, no fn > 40 lines, no complexity > 10.
- [x] No references to HIR interpreter, Taffy, or `render_tsx`.
- [x] Docs accurately describe rquickjs + Yoga architecture.

### Completed ✅ Phase 6–7 (Examples + HIR Expansion)
- [x] `spec_expressions` + `spec_types` modules enabled (Task 041).
- [x] 27 Ink examples added covering core TS/TSX/React/Ink features (Tasks 042–067).
- [x] Each example renders **identically** in deno, `runts dev`, and `runts build` (100% match).
- [x] Optional chaining `?.` parses into HIR (Task 068).
- [x] `as`, `satisfies`, `!` parse into HIR and are erased (Task 069).
- [x] Enums parse into HIR and codegen produces compilable Rust (Task 070).
- [x] Private fields `#field` parse into HIR and produce compilable Rust (Task 071).
- [x] Generators `function*` parse into HIR and map to Rust iterators (Task 072).
- [x] Dynamic imports `import()` parse into HIR (Task 073).
- [x] `tests/compile_codegen.rs` has 53 tests (Task 075).

### Pending 🔄 Phase 8–11
- [ ] Decorators parse into HIR without producing Invalid (Task 074).
- [ ] Compile-path parity tests for all examples (Task 076).
- [ ] Compile-path negative tests (Task 077).
- [ ] Coverage matrix published in `docs/SUPPORTED_SUBSET.md` (Task 078).
- [ ] 21 additional Ink examples for extended TS/TSX coverage (Tasks 079–099).
- [ ] 21 additional Ink examples for type system + runtime API deep coverage (Tasks 100–120).
- [ ] `scripts/parity.sh --env all` passes all examples with 100% match.
