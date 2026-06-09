# runts-ink: Execution Guide

> **Architecture:** rquickjs (dev engine) + Yoga (layout) + Ratatui (render).
> **HIR interpreter:** DELETED. Do not restore.
> **Taffy:** REMOVED. Yoga is the sole layout engine.
> **Goal:** 100% look&feel parity across 3 environments for all Ink examples, and maximum TS/TSX coverage in HIR + compile-path codegen.
> **Parity standard:** 100% output match. Zero divergence between deno, `runts dev`, and `runts build`.
> **Current stats:** 407 tasks, 176 completed, 231 pending, 34 phases, 343 example tasks.

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

## Coverage Strategy: Maximum TS/TSX in HIR + Compile-Path Codegen

The project maintains **two independent validation surfaces** for every TS/TSX feature:

1. **HIR representation** (`crates/runts-hir/`): The typed AST must be able to represent the feature without collapsing to `Expr::Invalid` or `Stmt::Invalid`.
2. **Compile-path codegen** (`src/transpile/hir/quote_codegen*.inc`): The code generator must emit compilable Rust for the feature.
3. **Runtime semantics** (rquickjs bridge): The dev path must execute the feature with 100% parity to deno.

### Example-driven coverage

Every practical TS/TSX/React/Ink feature is exercised by a real Ink TUI example in `examples/ink-*/`. The example is the test. The parity harness (`scripts/parity.sh`) automatically validates the feature in all 3 environments.

**Rule:** If an example compiles in deno but fails in `runts build`, the HIR parser or codegen bug must be fixed as part of that task. The example is not allowed to be simplified.

### Feature-to-layer mapping

Each task file must explicitly state which HIR variants and codegen paths it exercises:

| Feature Category | HIR Layer | Codegen Layer | Example Task Pattern |
|------------------|-----------|---------------|----------------------|
| Expressions | `crates/runts-hir/src/expr.rs` | `quote_codegen_exprs.inc` | `ink-*-operator`, `ink-*-expression` |
| Statements | `crates/runts-hir/src/stmt.rs` | `quote_codegen_stmts.inc` | `ink-control-flow`, `ink-*-statement` |
| Classes | `crates/runts-hir/src/base.rs` `ClassMember` | `quote_codegen.rs` class handling | `ink-class-*`, `ink-*-class-*` |
| Types | Parser erasure; no runtime HIR needed | Type erasure in codegen | `ink-type-*`, `ink-*-types` |
| JSX | `crates/runts-hir/src/expr.rs` JSX variants | JSX element codegen + Ratatui plugin | `ink-jsx-*`, `ink-*-jsx-*` |
| Modules | `crates/runts-hir/src/stmt.rs` import/export | Module resolution + codegen | `ink-*-export*`, `ink-*-import*` |
| React hooks | `js_bundle/react_shim.rs` | Hook shim + bridge | `ink-use-*`, `ink-react-*` |
| Runtime APIs | Standard `Expr::Call` + globals | Runtime API codegen or bridge mapping | `ink-*-api`, `ink-node-*` |

### What "100% coverage" means

- **Parser coverage:** oxc_parser can parse the construct (handled by upstream).
- **HIR coverage:** The parser→HIR converter produces a typed HIR variant, not `Expr::Invalid`/`Stmt::Invalid`.
- **Codegen coverage:** `quote_codegen` emits compilable Rust for the HIR variant.
- **Runtime coverage:** The generated Rust (compile path) or JS bundle (dev path) executes with output identical to deno.

A feature is "complete" only when all four layers are validated by an Ink example passing parity in all 3 environments.

### Handling unsupported features

Some features are intentionally unsupported (e.g., `eval()`, `with`, legacy `escape/unescape`). For these:
- The task file must still exist.
- The example must call the real API (no fallback).
- The acceptance criteria must state the expected behavior: either compile-path emits a clear error, or the feature works in rquickjs with parity to deno.
- The gap is documented in `docs/SUPPORTED_SUBSET.md`, not hidden.

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

| Feature | Parser | HIR | Task |
|---------|--------|-----|------|
| Optional chaining `?.` | ✅ oxc | ✅ Added | 068 |
| `as` / `satisfies` / `!` | ✅ oxc | ✅ Added | 069 |
| Enum declarations | ✅ oxc | ✅ Added | 070 |
| Private fields `#field` | ✅ oxc | ✅ Added | 071 |
| Generators `function*` | ✅ oxc | ✅ Added | 072 |
| Dynamic import `import()` | ✅ oxc | ✅ Added | 073 |
| Decorators | ✅ oxc | ✅ Added | 074 |

**Tasks:** 068–074 | **Status:** ✅ Completed (7/7)

---

### Phase 8: Compile-Path Integration Tests
**Goal:** The compile path is thoroughly tested with real generated code.

**Tasks:** 075–077 | **Status:** ✅ Completed (3/3)

---

### Phase 9: Final Audit
**Goal:** Document the exact coverage matrix. Set v1.0 targets.

**Tasks:** 078 | **Status:** ✅ Completed (1/1)

---

### Phase 10: Extended TS/TSX Coverage
**Goal:** Extended TS/TSX/React/Ink features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 079 | `ink-logical-assign` | `\|\|=`, `&&=`, `??=` | ✅ |
| 080 | `ink-use-layout-effect` | `useLayoutEffect` | ✅ |
| 081 | `ink-use-id-transition` | `useId`, `useTransition` | ✅ |
| 082 | `ink-type-alias-interface` | Type aliases, interfaces | ✅ |
| 083 | `ink-access-modifiers` | `public`/`private`/`protected`/`readonly` | ✅ |
| 084 | `ink-top-level-await` | Top-level `await` | ✅ |
| 085 | `ink-import-export-type` | `import type`, `export type` | ✅ |
| 086 | `ink-barrel-export` | `export * from`, `import * as` | ✅ |
| 087 | `ink-keyof-readonly` | `keyof`, `readonly` arrays | ✅ |
| 088 | `ink-bigint-globalthis` | BigInt, numeric separators, `globalThis` | ✅ |
| 089 | `ink-symbol-collections` | Symbol, Map, Set, WeakMap | ✅ |
| 090 | `ink-suspense-lazy` | `Suspense`, `lazy` | ✅ |
| 091 | `ink-error-boundary` | `<ErrorBoundary>` | ✅ |
| 092 | `ink-namespace-declare` | `namespace`, `declare` | ✅ |
| 093 | `ink-override-implements` | `override`, `implements` | ✅ |
| 094 | `ink-abstract-class` | `abstract` classes | ✅ |
| 095 | `ink-new-target` | `new.target` | ✅ |
| 096 | `ink-reflect-api` | `Reflect` API | ✅ |
| 097 | `ink-template-literal-types` | Template literal types | ✅ |
| 098 | `ink-infer-conditional` | `infer` in conditional types | ✅ |
| 099 | `ink-regexp-advanced` | RegExp flags, `matchAll` | ✅ |

**Tasks:** 079–099 | **Status:** ✅ Completed (21/21)

---

### Phase 11: Type System + Runtime API Deep Coverage
**Goal:** Type system deep features and modern runtime APIs exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 100 | `ink-utility-types` | `Partial`, `Required`, `Pick`, `Omit`, `Record`, `ReturnType` | ✅ |
| 101 | `ink-as-const` | `as const`, literal types, tuple types | ✅ |
| 102 | `ink-index-intersection` | Index signatures, intersection types (`A & B`) | ✅ |
| 103 | `ink-unknown-never` | `unknown`, `never`, user-defined type guards | ✅ |
| 104 | `ink-array-modern` | `flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `includes`, `findLast` | ✅ |
| 105 | `ink-object-modern` | `fromEntries`, `hasOwn`, `getOwnPropertyDescriptors` | ✅ |
| 106 | `ink-use-imperative-handle` | `useImperativeHandle`, `forwardRef` | ✅ |
| 107 | `ink-use-sync-external-store` | `useSyncExternalStore`, `useDeferredValue` | ✅ |
| 108 | `ink-context-advanced` | Context `displayName`, `defaultValue`, multiple providers | ✅ |
| 109 | `ink-discriminated-unions` | Discriminated unions, exhaustive checks | ✅ |
| 110 | `ink-mapped-types` | Mapped types `{ [K in T]: V }` | ✅ |
| 111 | `ink-proxy` | `Proxy` handler | ✅ |
| 112 | `ink-weakref` | `WeakRef`, `FinalizationRegistry` | ✅ |
| 113 | `ink-string-modern` | `padStart`, `padEnd`, `replaceAll`, `trimStart`, `trimEnd`, `at` | ✅ |
| 114 | `ink-promise-advanced` | `allSettled`, `any`, `race`, `withResolvers` | ✅ |
| 115 | `ink-this-parameter` | `this` parameter, `this` types | ✅ |
| 116 | `ink-unique-symbol` | `unique symbol`, branded types | ✅ |
| 117 | `ink-react-children` | `Children` API, `cloneElement`, `isValidElement` | ✅ |
| 118 | `ink-date-math` | `Date`, `Math`, `Intl` | ✅ |
| 119 | `ink-export-equal` | `export =`, `import = require()` | ✅ |
| 120 | `ink-global-augmentation` | `declare global`, `declare module` | ✅ |

**Tasks:** 100–120 | **Status:** ✅ Completed (21/21)

---

### Phase 12: Runtime API + Type System Completion
**Goal:** Final runtime APIs and type system features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 121 | `ink-json-api` | `JSON.stringify`, `JSON.parse` | ✅ |
| 122 | `ink-function-bind` | `bind`, `call`, `apply` | ✅ |
| 123 | `ink-object-meta` | `create`, `defineProperty`, `freeze`, `seal`, `assign` | ✅ |
| 124 | `ink-queue-microtask` | `queueMicrotask` | ✅ |
| 125 | `ink-error-cause` | `Error` with `cause` | ✅ |
| 126 | `ink-static-block` | Class `static {}` blocks | ✅ |
| 127 | `ink-private-methods` | Private methods `#method()`, `#field in obj` | ✅ |
| 128 | `ink-number-static` | `Number.isFinite`, `isNaN`, `parseInt`, `parseFloat`, `EPSILON` | ✅ |
| 129 | `ink-array-immutable` | `toSpliced`, `with` (ES2023) | ✅ |
| 130 | `ink-react-refs-debug` | `createRef`, `useDebugValue` | ✅ |
| 131 | `ink-function-overloads` | Function overloads | ✅ |

**Tasks:** 121–131 | **Status:** ✅ Completed (11/11)

---

### Phase 13: Real-World Validation (`../tui1` Port)
**Goal:** The `../tui1` real-world Ink chat example compiles and renders identically in all 3 environments with 100% output match.

| Task | Gap | Status |
|------|-----|--------|
| 132 | Port `../tui1` — comprehensive feature audit | ✅ |
| 133 | `process` global (`process.exit`, `process.env`, `process.stdin`, `process.stdout`) | ✅ |
| 134 | `setInterval` / `clearInterval` | ✅ |
| 135 | `Date` object (`new Date`, `Date.now`, `toLocaleTimeString`) | ✅ |
| 136 | `Array.prototype.splice` | ✅ |
| 137 | React Fragment shorthand `<></>` | ✅ |
| 138 | Dynamic import of node built-ins (`import("node:readline")`) | ✅ |
| 139 | `/** @jsxImportSource react */` pragma | ✅ |
| 140 | Module-level `render()` call | ✅ |

**Tasks:** 132–140 | **Status:** ✅ Completed (9/9)

---

### Phase 14: Final Runtime API + Module Pattern Completion
**Goal:** Remaining runtime APIs and module patterns exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 141 | `ink-namespace-reexport` | `export * as ns from "mod"` | ✅ |
| 142 | `ink-inline-type-import` | `import { type X }`, `import type * as ns` | ✅ |
| 143 | `ink-parameter-properties` | `constructor(public x: string)` | ✅ |
| 144 | `ink-console-methods` | `console.log`, `error`, `warn`, `info`, `time`, `timeEnd` | ✅ |
| 145 | `ink-uri-encoding` | `encodeURI`, `decodeURI`, `encodeURIComponent`, `decodeURIComponent` | ✅ |
| 146 | `ink-array-reduce` | `reduce`, `reduceRight` | ✅ |
| 147 | `ink-string-search` | `startsWith`, `endsWith`, `includes`, `repeat` | ✅ |
| 148 | `ink-error-subclasses` | `TypeError`, `RangeError`, `ReferenceError` | ✅ |

**Tasks:** 141–148 | **Status:** ✅ Completed (8/8)

---

### Phase 15: Advanced React Patterns + Deep Runtime Coverage
**Goal:** Advanced React patterns and remaining runtime APIs exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 149 | `ink-asserts-predicate` | `asserts` type predicate | ✅ |
| 150 | `ink-import-types` | `type T = import("./mod").Type` | ✅ |
| 151 | `ink-reference-directive` | `/// <reference types="..." />` | ✅ |
| 152 | `ink-use-insertion-effect` | `useInsertionEffect` | ✅ |
| 153 | `ink-render-props` | Render props pattern | ✅ |
| 154 | `ink-hoc` | Higher-order components | ✅ |
| 155 | `ink-symbol-iterator` | `Symbol.iterator`, custom iterables | ✅ |
| 156 | `ink-arraybuffer` | `ArrayBuffer`, `Uint8Array`, `DataView` | ✅ |
| 157 | `ink-performance` | `performance.now()`, `performance.mark` | ✅ |
| 158 | `ink-structured-clone` | `structuredClone` | ✅ |
| 159 | `ink-arguments` | `arguments` object | ✅ |
| 160 | `ink-regexp-named-groups` | Named capture groups | ✅ |
| 161 | `ink-string-wellformed` | `isWellFormed`, `toWellFormed` | ✅ |
| 162 | `ink-key-prop` | `key` prop in lists and fragments | ✅ |
| 163 | `ink-ref-callback` | Callback refs, `useRef` with initial value | ✅ |

**Tasks:** 149–163 | **Status:** ✅ Completed (15/15)

---

### Phase 16: Operator + Syntax + Runtime API Completion
**Goal:** Final JavaScript operators, syntax features, and runtime APIs exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 164 | `ink-in-operator` | `in` operator (`prop in obj`) | ✅ |
| 165 | `ink-for-await-of` | `for await...of` async iteration | ✅ |
| 166 | `ink-object-entries-values` | `Object.entries`, `Object.values`, `Object.keys` | ✅ |
| 167 | `ink-array-static` | `Array.from`, `Array.of`, `Array.isArray` | ✅ |
| 168 | `ink-parse-global` | `parseInt`, `parseFloat`, `isNaN`, `isFinite` | ✅ |
| 169 | `ink-import-meta-url` | `import.meta.url` | ✅ |
| 170 | `ink-debugger-labeled` | `debugger`, labeled statements | ✅ |
| 171 | `ink-number-string-proto` | `Number.prototype.toFixed`, `String.prototype.charAt` etc | ✅ |
| 172 | `ink-array-mutators` | `push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse` | ✅ |
| 173 | `ink-array-searchers` | `indexOf`, `lastIndexOf`, `every`, `some`, `filter`, `find`, `findIndex` | ✅ |
| 174 | `ink-date-comprehensive` | `getTime`, `getFullYear`, `toISOString`, `toUTCString` | ✅ |
| 175 | `ink-regexp-test-exec` | `RegExp.prototype.test`, `exec` | ✅ |
| 176 | `ink-promise-resolve-reject` | `Promise.resolve`, `Promise.reject` | ✅ |
| 177 | `ink-void-comma-increment` | `void`, comma operator, `++`/`--` | ✅ |
| 178 | `ink-spread-call-destructure` | Spread in calls, destructuring in params/catch/for-of | ✅ |

**Tasks:** 164–178 | **Status:** ✅ Completed (15/15)

---

### Phase 17: ES2024 + TypeScript 5.0+ Feature Completion
**Goal:** Latest ECMAScript 2024 and TypeScript 5.0+ features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 179 | `ink-using-declaration` | `using` / `await using` (ES2024) | ✅ |
| 180 | `ink-import-attributes` | Import attributes (`with { type: "json" }`) | ✅ |
| 181 | `ink-array-from-async` | `Array.fromAsync` | ✅ |
| 182 | `ink-promise-with-resolvers` | `Promise.withResolvers` | ✅ |
| 183 | `ink-object-group-by` | `Object.groupBy` / `Map.groupBy` | ✅ |
| 184 | `ink-const-type-param` | `const T` type parameters (TS 5.0) | ✅ |
| 185 | `ink-accessor-field` | `accessor` class fields (TS 5.0) | 🔄 |
| 186 | `ink-symbol-async-iterator` | `Symbol.asyncIterator` | ⏳ |
| 187 | `ink-throw-expression` | Throw expressions (Stage 3) | ⏳ |
| 188 | `ink-type-annotation-catch` | Type annotation in `catch` clause | ⏳ |
| 189 | `ink-satisfies-expr` | `satisfies` in object/array literals | ⏳ |
| 190 | `ink-iterator-helpers` | Iterator helpers (`map`, `filter`, `take`) | ⏳ |

**Tasks:** 179–190 | **Status:** 🔄 6/12 completed

---

### Phase 18: Expression-Level + React Pattern + Runtime API Completion
**Goal:** TS/TSX expression-level features, React type patterns, advanced runtime APIs, JSX edge cases, and legacy features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 191 | `ink-class-expression` | Anonymous class expressions | ⏳ |
| 192 | `ink-function-expression` | Anonymous/named function expressions | ⏳ |
| 193 | `ink-react-fc-type` | `React.FC`, `React.FunctionComponent` | ⏳ |
| 194 | `ink-props-with-children` | `React.PropsWithChildren` | ⏳ |
| 195 | `ink-string-advanced` | `localeCompare`, `normalize`, `codePointAt`, `fromCodePoint`, `concat`, `charAt`, `charCodeAt` | ⏳ |
| 196 | `ink-array-advanced` | `findLast`, `findLastIndex`, `fill`, `copyWithin`, `at` | ✅ |
| 197 | `ink-ts-directive` | `// @ts-expect-error`, `// @ts-ignore` | ⏳ |
| 198 | `ink-dangerously-set-inner-html` | `dangerouslySetInnerHTML` JSX prop | ⏳ |
| 199 | `ink-default-props` | `defaultProps` on function components | ⏳ |
| 200 | `ink-class-lifecycle` | `componentDidMount`, `componentWillUnmount` | ⏳ |
| 201 | `ink-get-derived-state-from-error` | `static getDerivedStateFromError`, `componentDidCatch` | ⏳ |
| 202 | `ink-export-star-as` | `export * as ns from "mod"` | ⏳ |
| 203 | `ink-with-statement` | `with` statement (legacy) | ⏳ |
| 204 | `ink-jsx-spread-attribute` | JSX spread attributes `{...props}` | ⏳ |
| 205 | `ink-set-prototype` | `Set`/`Map`/`WeakMap`/`WeakSet` prototype methods | ⏳ |

**Tasks:** 191–205 | **Status:** 🔄 1/15 completed

---

### Phase 19: Tagged Templates + Compiler Options + Reflect Deep Coverage + ES2025
**Goal:** Advanced expression patterns, TypeScript compiler options, deep runtime API coverage, and ES2025 features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 206 | `ink-tagged-template` | Tagged template literals | ⏳ |
| 207 | `ink-function-constructor` | `Function` constructor | ⏳ |
| 208 | `ink-object-prototype` | Object prototype methods (`toString`, `valueOf`, `hasOwnProperty`, etc.) | ⏳ |
| 209 | `ink-object-static` | Object static methods (`is`, `setPrototypeOf`, `getPrototypeOf`, etc.) | ⏳ |
| 210 | `ink-reflect-deep` | Reflect methods (`apply`, `construct`, `defineProperty`, etc.) | ⏳ |
| 211 | `ink-tsconfig-paths` | tsconfig `paths` mapping | ⏳ |
| 212 | `ink-esmodule-interop` | `esModuleInterop`, `allowSyntheticDefaultImports` | ⏳ |
| 213 | `ink-jsx-factory-pragma` | `jsxFactory`, `jsxFragmentFactory`, `jsxImportSource` pragmas | ⏳ |
| 214 | `ink-strict-mode` | TypeScript strict mode flags | ⏳ |
| 215 | `ink-downlevel-iteration` | `downlevelIteration` for ES5 targets | ⏳ |
| 216 | `ink-preserve-const-enum` | `preserveConstEnums` behavior | ⏳ |
| 217 | `ink-module-resolution` | `moduleResolution` modes (`node`, `bundler`, `classic`) | ⏳ |
| 218 | `ink-web-api` | Web APIs (`URL`, `TextEncoder`, `Headers`, etc.) | ⏳ |
| 219 | `ink-regexp-escape` | `RegExp.escape` (ES2025) | ⏳ |
| 220 | `ink-set-es2025` | Set methods (`intersection`, `union`, `isSubsetOf`, etc.) | ⏳ |

**Tasks:** 206–220 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 20: Advanced Language + Runtime + React Patterns + Compile Infrastructure
**Goal:** Advanced language features (eval, super calls, private-in, import.meta.resolve, globalThis, BigInt), runtime API deep coverage (Intl, Atomics), compile path infrastructure (source maps, verbatimModuleSyntax), JSX advanced patterns (member expressions, namespaces), and React patterns (startTransition, React 19 hooks) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 221 | `ink-eval` | `eval()` function | ⏳ |
| 222 | `ink-super-call` | `super()` in class constructors | ⏳ |
| 223 | `ink-private-in` | `#field in obj` (private identifier in `in`) | ⏳ |
| 224 | `ink-import-meta-resolve` | `import.meta.resolve` | ⏳ |
| 225 | `ink-global-this` | `globalThis` | ⏳ |
| 226 | `ink-bigint-ops` | BigInt literals and operations | ⏳ |
| 227 | `ink-intl-datetime` | `Intl.DateTimeFormat` | ⏳ |
| 228 | `ink-intl-number` | `Intl.NumberFormat` | ⏳ |
| 229 | `ink-atomics` | `Atomics` API | ⏳ |
| 230 | `ink-source-map` | Source map generation | ⏳ |
| 231 | `ink-verbatim-module-syntax` | `verbatimModuleSyntax` (TS 5.0) | ⏳ |
| 232 | `ink-jsx-member` | JSX member expressions (`My.Component`) | ⏳ |
| 233 | `ink-jsx-namespace` | JSX namespaced elements (`ns:tag`) | ⏳ |
| 234 | `ink-start-transition` | `startTransition` | ⏳ |
| 235 | `ink-react-19-hooks` | `useFormStatus`, `useOptimistic`, `useActionState`, `use` | ⏳ |

**Tasks:** 221–235 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 21: Niche Language Features + TypeScript Config + Web Streams + Crypto
**Goal:** Niche language features (sequence expressions, labeled break/continue, nested/default destructuring, readonly tuples, keyof typeof), TypeScript configuration (reference path, isolatedModules, resolveJsonModule), and runtime APIs (AggregateError, URLSearchParams, TextDecoder, Web Crypto, ReadableStream, CompressionStream) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 236 | `ink-sequence-expression` | Comma operator sequences | ⏳ |
| 237 | `ink-labeled-break-continue` | Labeled `break` and `continue` | ⏳ |
| 238 | `ink-nested-destructure` | Nested object/array destructuring | ⏳ |
| 239 | `ink-default-destructure` | Default values in destructuring | ⏳ |
| 240 | `ink-readonly-tuple` | Readonly tuples and arrays | ⏳ |
| 241 | `ink-keyof-typeof` | `keyof typeof` pattern | ⏳ |
| 242 | `ink-reference-path` | `/// <reference path="..." />` | ⏳ |
| 243 | `ink-isolated-modules` | `isolatedModules` compiler option | ⏳ |
| 244 | `ink-resolve-json-module` | `resolveJsonModule` | ⏳ |
| 245 | `ink-aggregate-error` | `AggregateError` | ⏳ |
| 246 | `ink-url-search-params` | `URLSearchParams` | ⏳ |
| 247 | `ink-text-decoder` | `TextDecoder` | ⏳ |
| 248 | `ink-crypto-random` | `crypto.randomUUID` | ⏳ |
| 249 | `ink-readable-stream` | `ReadableStream` | ⏳ |
| 250 | `ink-compression-stream` | `CompressionStream` / `DecompressionStream` | ⏳ |

**Tasks:** 236–250 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 22: Web APIs + TypeScript Config Edge Cases + JSX/TS Language Patterns
**Goal:** Web APIs (WebSocket, FormData, Blob, EventTarget, FinalizationRegistry, SharedArrayBuffer), TypeScript configuration edge cases (noImplicitReturns, noFallthroughCasesInSwitch, noUncheckedIndexedAccess, exactOptionalPropertyTypes, preserveValueImports, jsxPreserve, emitDecoratorMetadata), and advanced TS/JSX language patterns (JSX spread children, rest destructuring, named tuples, const enum, ambient modules, ts-nocheck, server components) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 251 | `ink-websocket` | WebSocket API | ⏳ |
| 252 | `ink-form-data` | FormData API | ⏳ |
| 253 | `ink-blob` | Blob and FileReader APIs | ⏳ |
| 254 | `ink-event-target` | EventTarget and CustomEvent | ⏳ |
| 255 | `ink-finalization-registry` | FinalizationRegistry | ⏳ |
| 256 | `ink-shared-array-buffer` | SharedArrayBuffer | ⏳ |
| 257 | `ink-no-implicit-returns` | `noImplicitReturns` compiler option | ⏳ |
| 258 | `ink-no-fallthrough` | `noFallthroughCasesInSwitch` compiler option | ⏳ |
| 259 | `ink-no-unchecked-indexed` | `noUncheckedIndexedAccess` compiler option | ⏳ |
| 260 | `ink-exact-optional` | `exactOptionalPropertyTypes` compiler option | ⏳ |
| 261 | `ink-preserve-value-imports` | `preserveValueImports` compiler option | ⏳ |
| 262 | `ink-jsx-spread-children` | Spread in JSX children | ⏳ |
| 263 | `ink-rest-destructure` | Rest elements in object/array destructuring | ⏳ |
| 264 | `ink-named-tuple` | Named tuple members | ⏳ |
| 265 | `ink-const-enum` | `const enum` declaration | ⏳ |
| 266 | `ink-ambient-module` | Ambient module declarations | ⏳ |
| 267 | `ink-ts-nocheck` | `// @ts-nocheck` and `// @ts-check` directives | ⏳ |
| 268 | `ink-jsx-preserve` | JSX `preserve` transform | ⏳ |
| 269 | `ink-server-component` | `"use server"` / `"use client"` directives | ⏳ |
| 270 | `ink-decorator-metadata` | `emitDecoratorMetadata` | ⏳ |

**Tasks:** 251–270 | **Status:** ⏳ Pending (20 tasks)

---

### Phase 23: Emerging Runtime APIs + React Patterns + Module Resolution + Compile Infrastructure
**Goal:** Emerging runtime APIs (Temporal, additional Intl formatters, SuppressedError, String.raw, AsyncGenerator, Iterator.from, DisposableStack, escape/unescape), React patterns (Profiler, StrictMode, createPortal), module resolution (package.json exports/imports, project references), and compile infrastructure (watch mode, declaration files) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 271 | `ink-temporal-api` | Temporal API | ⏳ |
| 272 | `ink-intl-list` | `Intl.ListFormat`, `RelativeTimeFormat`, `PluralRules`, `Collator` | ⏳ |
| 273 | `ink-suppressed-error` | `SuppressedError`, `Error.isError` | ⏳ |
| 274 | `ink-string-raw` | `String.raw` | ⏳ |
| 275 | `ink-async-generator` | Async generators | ⏳ |
| 276 | `ink-iterator-from` | `Iterator.from` | ⏳ |
| 277 | `ink-disposable-stack` | `DisposableStack`, `AsyncDisposableStack` | ⏳ |
| 278 | `ink-escape-unescape` | Legacy `escape()`, `unescape()` | ⏳ |
| 279 | `ink-react-profiler` | `React.Profiler` | ⏳ |
| 280 | `ink-react-strict-mode` | `React.StrictMode` | ⏳ |
| 281 | `ink-react-portal` | `ReactDOM.createPortal` | ⏳ |
| 282 | `ink-package-exports` | `package.json` `exports`/`imports` | ⏳ |
| 283 | `ink-project-references` | TypeScript project references | ⏳ |
| 284 | `ink-watch-mode` | Watch mode / incremental compilation | ⏳ |
| 285 | `ink-declaration-files` | `.d.ts` declaration files | ⏳ |

**Tasks:** 271–285 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 24: Orphaned Example Audit + Advanced Type System + Language Edge Cases
**Goal:** Comprehensive audit of 92 orphaned existing Ink examples, plus advanced type system features (recursive types, distributive conditional types, variadic tuples, branded types), language edge cases (generator return/throw, optional catch binding, nullish in objects, object shorthand, computed class members, class field initializers), and module/type directives (type-only import assertions, AMD module, satisfies on functions) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 286 | — | Audit and track 92 orphaned existing Ink examples | ⏳ |
| 287 | `ink-recursive-types` | Recursive type aliases/interfaces | ⏳ |
| 288 | `ink-distributive-conditional` | Distributive conditional types | ⏳ |
| 289 | `ink-variadic-tuple` | Variadic tuple types | ⏳ |
| 290 | `ink-generator-return-throw` | `generator.return()` / `generator.throw()` | ⏳ |
| 291 | `ink-branded-types` | Branded / opaque types | ⏳ |
| 292 | `ink-type-only-import-assertion` | `import type` with import attributes | ⏳ |
| 293 | `ink-amd-module` | `/// <reference amd-module name="..." />` | ⏳ |
| 294 | `ink-satisfies-function` | `satisfies` on function expressions | ⏳ |
| 295 | `ink-object-pattern-shorthand` | Shorthand properties and method syntax | ⏳ |
| 296 | `ink-computed-class-members` | Computed property keys in classes | ⏳ |
| 297 | `ink-class-fields-init` | Class fields with complex initializers | ⏳ |
| 298 | `ink-optional-catch-binding` | Optional catch binding | ⏳ |
| 299 | `ink-nullish-in-object` | Nullish values in object literals | ⏳ |

**Tasks:** 286–299 | **Status:** ⏳ Pending (14 tasks)

---

### Phase 25: Advanced Destructuring + Node.js/Web Runtime APIs
**Goal:** Advanced destructuring patterns (mixed array/object, computed keys, renaming), Node.js runtime APIs (global, Buffer, __dirname, __filename, CommonJS interop, process.nextTick, EventEmitter, path, os), and Web APIs (AbortSignal, fetch, PerformanceObserver, setImmediate, URLPattern) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 300 | `ink-mixed-destructure` | Mixed array/object destructuring | ⏳ |
| 301 | `ink-computed-destructure` | Computed keys and renaming in destructuring | ⏳ |
| 302 | `ink-global-object` | `global` object | ⏳ |
| 303 | `ink-buffer` | Node.js `Buffer` | ⏳ |
| 304 | `ink-dirname-filename` | `__dirname` and `__filename` | ⏳ |
| 305 | `ink-commonjs-interop` | `module.exports`, `exports`, `require` | ⏳ |
| 306 | `ink-abort-signal` | `AbortSignal` and `AbortController` | ⏳ |
| 307 | `ink-fetch-api` | `fetch`, `Response`, `Request` | ⏳ |
| 308 | `ink-performance-observer` | `PerformanceObserver`, `mark`, `measure` | ⏳ |
| 309 | `ink-set-immediate` | `setImmediate` / `clearImmediate` | ⏳ |
| 310 | `ink-process-nexttick` | `process.nextTick` | ⏳ |
| 311 | `ink-events-emitter` | `EventEmitter` | ⏳ |
| 312 | `ink-path-module` | `path` module | ⏳ |
| 313 | `ink-os-module` | `os` module | ⏳ |
| 314 | `ink-urlpattern` | `URLPattern` | ⏳ |

**Tasks:** 300–314 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 26: Node.js Standard Library + Browser Globals + Binary Data + Promise/Math Completion
**Goal:** Node.js standard library modules (fs, crypto, util, stream, readline, assert, child_process, http), process properties (pid, cwd, uptime), browser globals (window, document, navigator, localStorage), binary data APIs (typed arrays, DataView, File), Promise chaining (then/catch/finally), and advanced Math methods/constants exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 315 | `ink-node-fs` | Node.js `fs` module | ⏳ |
| 316 | `ink-node-crypto` | Node.js `crypto` module | ⏳ |
| 317 | `ink-node-util` | Node.js `util` module | ⏳ |
| 318 | `ink-node-stream` | Node.js `stream` module | ⏳ |
| 319 | `ink-node-readline` | Node.js `readline` module | ⏳ |
| 320 | `ink-node-assert` | Node.js `assert` module | ⏳ |
| 321 | `ink-node-child-process` | Node.js `child_process` module | ⏳ |
| 322 | `ink-node-http` | Node.js `http`/`https` module | ⏳ |
| 323 | `ink-process-props` | `process` properties (pid, cwd, uptime, hrtime, memoryUsage) | ⏳ |
| 324 | `ink-browser-globals` | `window`, `document`, `navigator`, `location` | ⏳ |
| 325 | `ink-web-storage` | `localStorage` / `sessionStorage` | ⏳ |
| 326 | `ink-typed-arrays` | Typed arrays and `DataView` | ⏳ |
| 327 | `ink-file-constructor` | `File` constructor | ⏳ |
| 328 | `ink-promise-chain` | `Promise.prototype.then/catch/finally` + `Promise.any`/`race` | ⏳ |
| 329 | `ink-math-advanced` | Advanced `Math` methods and constants | ⏳ |

**Tasks:** 315–329 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 27: React Hook Patterns + JSX Expression Patterns + Component Composition
**Goal:** React hook patterns (useEffect cleanup, useCallback/useMemo dependencies, custom hook composition), React children API (Children.only, Children.toArray), component composition patterns (generic components, polymorphic components, compound components, context reducer, controlled/uncontrolled, forwardRef with generics, render props with generics, HOCs with generics), and JSX expression patterns (optional call, non-null assertion, type assertions, nullish coalescing, optional chaining, conditional rendering) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 330 | `ink-use-effect-cleanup` | `useEffect` cleanup functions | ⏳ |
| 331 | `ink-use-callback-deps` | `useCallback` with dependencies | ⏳ |
| 332 | `ink-use-memo-deps` | `useMemo` with dependencies | ⏳ |
| 333 | `ink-children-only` | `Children.only` | ⏳ |
| 334 | `ink-children-toarray` | `Children.toArray` | ⏳ |
| 335 | `ink-react-lazy-fallback` | `React.lazy` with `Suspense` fallback | ⏳ |
| 336 | `ink-generic-function-component` | Generic function components | ⏳ |
| 337 | `ink-polymorphic-component` | Polymorphic components with `as` prop | ⏳ |
| 338 | `ink-compound-components` | Compound component pattern | ⏳ |
| 339 | `ink-context-reducer` | Context with `useReducer` | ⏳ |
| 340 | `ink-controlled-uncontrolled` | Controlled vs uncontrolled components | ⏳ |
| 341 | `ink-forward-ref-generic` | `forwardRef` with generic components | ⏳ |
| 342 | `ink-render-props-generic` | Render props with generics | ⏳ |
| 343 | `ink-hoc-generic` | Higher-order components with generics | ⏳ |
| 344 | `ink-custom-hook-composition` | Composing multiple custom hooks | ⏳ |
| 345 | `ink-optional-call` | Optional call expression (`fn?.()`) | ⏳ |
| 346 | `ink-non-null-chain` | Non-null assertion after optional chain | ⏳ |
| 347 | `ink-type-assertion-jsx` | Type assertions in JSX expressions | ⏳ |
| 348 | `ink-nullish-jsx-attr` | Nullish coalescing in JSX attributes | ⏳ |
| 349 | `ink-optional-chain-jsx` | Optional chaining in JSX attributes | ⏳ |
| 350 | `ink-jsx-conditional-render` | Ternary / logical AND returning JSX | ⏳ |

**Tasks:** 330–350 | **Status:** ⏳ Pending (21 tasks)

---

### Phase 28: Advanced Type System Patterns
**Goal:** Advanced TypeScript type system patterns exercised by Ink examples: mapped types with `as` clause, template literal types with unions, user-defined type guards (`is`), `satisfies` with unions, `as const` in function returns, utility types `Extract`/`Exclude`/`NonNullable`, key remapping, recursive readonly mapped types, nested conditional types, and inline `import('...').Type` syntax, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 351 | `ink-mapped-types-as` | Mapped types with `as` clause | ⏳ |
| 352 | `ink-template-literal-union` | Template literal types with unions | ⏳ |
| 353 | `ink-type-guard-is` | User-defined type guards (`is`) | ⏳ |
| 354 | `ink-satisfies-union` | `satisfies` with union types | ⏳ |
| 355 | `ink-as-const-function` | `as const` in function returns | ⏳ |
| 356 | `ink-utility-extract-exclude` | `Extract`, `Exclude`, `NonNullable` | ⏳ |
| 357 | `ink-key-remapping` | Key remapping with mapped types | ⏳ |
| 358 | `ink-readonly-mapped` | Recursive readonly mapped types | ⏳ |
| 359 | `ink-nested-conditional` | Nested conditional types | ⏳ |
| 360 | `ink-inline-import-type` | Inline `import('...').Type` syntax | ⏳ |

**Tasks:** 351–360 | **Status:** ⏳ Pending (10 tasks)

---

### Phase 29: Array/Object/String/Number/Date/Console/Error/Symbol/Process/Stream/WebAssembly API Completion
**Goal:** Final Ink examples for array methods (flat, flatMap), object methods (fromEntries), string methods (match, search, replace), RegExp well-known symbols, number formatting (toExponential, toPrecision, toFixed), date locale formatting (toLocaleDateString, toLocaleTimeString, toDateString, toTimeString), advanced console methods (assert, count, group, trace, timeLog), error stack APIs (captureStackTrace, stackTraceLimit), well-known symbols (toStringTag, toPrimitive, hasInstance, species), import.meta.env (Vite pattern), process.hrtime, fs/promises, stream/web (ReadableStream, WritableStream, TransformStream), timers/promises, and WebAssembly, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 361 | `ink-array-flat-flatmap` | `Array.prototype.flat` and `flatMap` | ⏳ |
| 362 | `ink-object-fromentries` | `Object.fromEntries` | ⏳ |
| 363 | `ink-string-match-search` | `String.prototype.match`, `search`, `replace` | ⏳ |
| 364 | `ink-regexp-symbol` | RegExp well-known symbols | ⏳ |
| 365 | `ink-number-format` | `Number.prototype.toExponential`, `toPrecision`, `toFixed` | ⏳ |
| 366 | `ink-date-locale` | Date locale formatting methods | ⏳ |
| 367 | `ink-console-advanced` | `console.assert`, `count`, `group`, `trace`, `timeLog` | ⏳ |
| 368 | `ink-error-stack` | `Error.captureStackTrace`, `Error.stackTraceLimit` | ⏳ |
| 369 | `ink-symbol-wellknown` | Well-known symbols | ⏳ |
| 370 | `ink-import-meta-env` | `import.meta.env` (Vite pattern) | ⏳ |
| 371 | `ink-process-hrtime` | `process.hrtime` / `process.hrtime.bigint` | ⏳ |
| 372 | `ink-fs-promises` | `fs/promises` module | ⏳ |
| 373 | `ink-stream-web` | `stream/web` (ReadableStream, WritableStream, TransformStream) | ⏳ |
| 374 | `ink-timers-promises` | `timers/promises` module | ⏳ |
| 375 | `ink-webassembly` | `WebAssembly` API | ⏳ |

**Tasks:** 361–375 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 30: Final Comprehensive Coverage Audit
**Goal:** End-to-end audit of all 407 tasks, verifying file-to-JSON consistency, phase coverage, 100% parity language, orphaned example tracking, and final stats publication.

| Task | Description | Status |
|------|-------------|--------|
| 376 | Final comprehensive coverage audit | ⏳ |

**Tasks:** 376 | **Status:** ⏳ Pending (1 task)

---

### Phase 31: Advanced JSX + React Edge Cases
**Goal:** Edge-case JSX and React patterns not yet explicitly exercised: JSX generic type arguments, full Children API (forEach/map/count), string manipulation utility types (Uppercase/Lowercase/Capitalize/Uncapitalize), advanced built-in utilities (Awaited/InstanceType/ConstructorParameters/ThisType), global declare var augmentation, inline type imports, type re-exports, multiple interface inheritance, typeof on classes, keyof on arrays, for...of with entries/keys/values, switch fallthrough, destructuring with rest and defaults, yield* with iterables, and try...finally without catch.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 377 | `ink-jsx-generic-type-args` | JSX generic type arguments | ⏳ |
| 378 | `ink-react-children-foreach-map` | `Children.forEach`, `Children.map`, `Children.count` | ⏳ |
| 379 | `ink-ts-string-manipulation-types` | `Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize` | ⏳ |
| 380 | `ink-ts-utility-types-advanced` | `Awaited`, `InstanceType`, `ConstructorParameters`, `ThisType` | ⏳ |
| 381 | `ink-ts-global-var-augmentation` | `declare var` global augmentation | ⏳ |
| 382 | `ink-import-type-inline` | `import type { type X }` (TS 4.5+) | ⏳ |
| 383 | `ink-export-type-reexport` | `export type { X } from './module'` | ⏳ |
| 384 | `ink-interface-multiple-extends` | `interface extends A, B, C` | ⏳ |
| 385 | `ink-typeof-class` | `typeof` on class constructors | ⏳ |
| 386 | `ink-keyof-array` | `keyof` on arrays and tuples | ⏳ |
| 387 | `ink-forof-entries` | `for...of` with `.entries()`, `.keys()`, `.values()` | ⏳ |
| 388 | `ink-switch-fallthrough` | `switch` with intentional fallthrough | ⏳ |
| 389 | `ink-destructure-rest-defaults` | Destructuring with rest and defaults combined | ⏳ |
| 390 | `ink-yield-star-iterable` | `yield*` with arrays, strings, maps, sets | ⏳ |
| 391 | `ink-try-finally-only` | `try...finally` without `catch` | ⏳ |

**Tasks:** 377–391 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 32: Core Language + React + Runtime Edge Cases
**Goal:** Remaining practical TS/TSX/React/Ink edge cases: IIFE, function declarations, block scoping, object shorthand/getters/setters/computed properties, JSX boolean attributes, Promise.all mixed outcomes, Array.flat(Infinity), JSON.stringify replacer, console.table, useReducer init, forwardRef displayName, parseInt radix, sparse arrays.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 392 | `ink-iife` | Immediately Invoked Function Expressions | ⏳ |
| 393 | `ink-function-declaration` | Named function declarations | ⏳ |
| 394 | `ink-block-scope` | `let` vs `var` vs `const` scoping | ⏳ |
| 395 | `ink-shorthand-properties` | Object shorthand and method shorthand | ⏳ |
| 396 | `ink-object-getters-setters` | Getter and setter properties | ⏳ |
| 397 | `ink-computed-properties` | Computed property names `[expr]: value` | ⏳ |
| 398 | `ink-jsx-boolean-attrs` | JSX boolean attributes | ⏳ |
| 399 | `ink-promise-all-mixed` | Promise.all with mixed resolve/reject | ⏳ |
| 400 | `ink-array-flat-infinity` | Array.flat with Infinity depth | ⏳ |
| 401 | `ink-json-stringify-replacer` | JSON.stringify with replacer function | ⏳ |
| 402 | `ink-console-table` | console.table with complex data | ⏳ |
| 403 | `ink-use-reducer-init` | useReducer with initialization function | ⏳ |
| 404 | `ink-forward-ref-displayname` | forwardRef with displayName | ⏳ |
| 405 | `ink-parse-int-radix` | parseInt with explicit radix | ⏳ |
| 406 | `ink-sparse-array` | Sparse arrays with holes `[1, , 3]` | ⏳ |

**Tasks:** 392–406 | **Status:** ⏳ Pending (15 tasks)

---

### Phase 33: HIR & Codegen Edge Cases
**Goal:** Additional TS/TSX/React/Ink features not yet explicitly exercised in HIR and compile-path codegen: super member access, fundamental Array/String methods, deep Object statics, console debug methods, process info, Symbol registry, Function properties, base64 encoding, localStorage full API, Web Crypto, Object prototype methods, RegExp compile, BigInt prototype, Intl locale/segmenter/displayNames, requestAnimationFrame, performance navigation, locale string formatting, defineProperties, and Date parse/setters.

| Task | Example | Feature | Status |
|------|---------|---------|--------|
| 407 | `ink-super-member` | `super.method()`, `super[prop]()` | ⏳ |
| 408 | `ink-array-fundamental` | `map`, `filter`, `forEach`, `concat`, `join`, `slice` | ⏳ |
| 409 | `ink-string-fundamental` | `indexOf`, `lastIndexOf`, `slice`, `split`, `substring`, `toLowerCase`, `toUpperCase`, `trim`, `matchAll` | ⏳ |
| 410 | `ink-object-static-deep` | `getOwnPropertyDescriptor`, `preventExtensions`, `isExtensible`, `isSealed`, `isFrozen`, `getOwnPropertyNames`, `getOwnPropertySymbols`, `defineProperties` | ⏳ |
| 411 | `ink-console-debug` | `debug`, `dir`, `groupEnd`, `countReset` | ⏳ |
| 412 | `ink-process-info` | `platform`, `version`, `versions`, `arch`, `release` | ⏳ |
| 413 | `ink-symbol-for` | `Symbol.for`, `Symbol.keyFor`, `Symbol.isConcatSpreadable`, `Symbol.unscopables` | ⏳ |
| 414 | `ink-function-props` | `length`, `name`, `prototype`, `[Symbol.hasInstance]` | ⏳ |
| 415 | `ink-atob-btoa` | `atob`, `btoa` | ⏳ |
| 416 | `ink-localstorage-api` | `getItem`, `setItem`, `removeItem`, `clear`, `length`, `key` | ⏳ |
| 417 | `ink-crypto-web` | `crypto.getRandomValues`, `crypto.subtle` | ⏳ |
| 418 | `ink-object-prototype-deep` | `toLocaleString`, `isPrototypeOf`, `propertyIsEnumerable` | ⏳ |
| 419 | `ink-regexp-compile` | `RegExp.prototype.compile`, `RegExp.prototype.toString` | ⏳ |
| 420 | `ink-bigint-proto` | `BigInt.prototype.toString`, `BigInt.prototype.valueOf` | ⏳ |
| 421 | `ink-intl-locale` | `Intl.Segmenter`, `Intl.DisplayNames`, `Intl.Locale` | ⏳ |
| 422 | `ink-request-animation-frame` | `requestAnimationFrame`, `cancelAnimationFrame` | ⏳ |
| 423 | `ink-performance-navigation` | `performance.timing`, `performance.navigation` | ⏳ |
| 424 | `ink-array-tolocalestring` | `Array.prototype.toLocaleString`, `Number.prototype.toLocaleString` | ⏳ |
| 425 | `ink-object-define-properties` | `Object.defineProperties` | ⏳ |
| 426 | `ink-date-parse-setters` | `Date.parse`, `Date.UTC`, `setTime`, `setFullYear`, `setMonth`, `setDate`, `setHours` | ⏳ |

**Tasks:** 407–426 | **Status:** ⏳ Pending (20 tasks)

---

## HIR Coverage Matrix

HIR must be able to represent every construct used in examples. This table maps feature categories to the relevant HIR files and current status.

| Category | HIR File | Key Variants | Coverage |
|----------|----------|--------------|----------|
| Literals | `crates/runts-hir/src/expr.rs` | `Number`, `String`, `Bool`, `Null`, `Undefined`, `BigInt`, `RegExp` | ✅ Complete |
| Operators | `crates/runts-hir/src/expr.rs` | `Unary`, `Binary`, `Logical`, `Conditional`, `Sequence`, `Assign`, `Update`, `Spread` | ✅ Complete |
| Member/Call | `crates/runts-hir/src/expr.rs` | `Member`, `ComputedMember`, `OptionalMember`, `Call`, `OptionalCall`, `New`, `SuperCall` | ✅ Complete |
| Functions | `crates/runts-hir/src/expr.rs` | `Arrow`, `Function`, `AsyncFunction`, `Generator` | ✅ Complete |
| Classes | `crates/runts-hir/src/base.rs` | `Class`, `ClassMember`, `MethodKind`, `is_private`, `is_static` | ✅ Complete |
| JSX | `crates/runts-hir/src/expr.rs` | `JsxElement`, `JsxFragment`, `JsxSpreadAttribute`, `JsxMember`, `JsxNamespace` | ✅ Complete |
| Modules | `crates/runts-hir/src/stmt.rs` | `Import`, `Export`, `ExportDefault`, `ExportAll` | ✅ Complete |
| Control Flow | `crates/runts-hir/src/stmt.rs` | `If`, `Switch`, `For`, `While`, `DoWhile`, `ForIn`, `ForOf`, `ForAwaitOf`, `TryCatch`, `Throw`, `Break`, `Continue`, `Labeled`, `Block`, `Return` | ✅ Complete |
| Declarations | `crates/runts-hir/src/stmt.rs` | `Var`, `Const`, `Let`, `FunctionDecl`, `Class`, `Enum`, `TypeAlias`, `Interface`, `Namespace`, `AmbientModule`, `Using`, `AwaitUsing` | ✅ Complete |
| Type Expressions | `crates/runts-hir/src/expr.rs` | `TypeAssertion`, `Satisfies`, `NonNull` | ✅ Complete |
| Async/Iter | `crates/runts-hir/src/expr.rs` | `Await`, `Yield`, `YieldFrom`, `ImportExpr`, `ImportMeta` | ✅ Complete |
| Patterns | `crates/runts-hir/src/base.rs` | `Pat::Ident`, `Pat::Array`, `Pat::Object`, `Pat::Default`, `Pat::Rest` | ✅ Complete |

**HIR coverage status:** All 40+ Expr variants and 26+ Stmt variants required by the task roadmap are represented. Pending: `Super` base in member expressions (Task 407), `JsxGenericTypeArgs` (Task 377), `AwaitUsing` (Task 179).

---

## Compile-Path Codegen Matrix

For each HIR variant, `quote_codegen` must emit compilable Rust. This table maps constructs to their codegen files.

| Construct | Codegen File | Status |
|-----------|--------------|--------|
| Literals | `quote_codegen_exprs.inc` | ✅ |
| Binary/Unary/Logical operators | `quote_codegen_exprs.inc` | ✅ |
| Member/Computed/Optional member | `quote_codegen_exprs.inc` | ✅ |
| Call/New/Optional call | `quote_codegen_exprs.inc` | ✅ |
| Template literals | `quote_codegen_exprs.inc` | ✅ |
| Tagged templates | `quote_codegen_exprs.inc` | ✅ |
| Arrow functions | `quote_codegen_exprs.inc` | ✅ |
| Function expressions | `quote_codegen_exprs.inc` | ✅ |
| Async/await | `quote_codegen_exprs.inc` | ✅ |
| Generators/yield | `quote_codegen_exprs.inc` | ✅ |
| Object literals (inc. getters/setters/methods) | `quote_codegen_exprs.inc` | ✅ |
| Array literals + spread | `quote_codegen_exprs.inc` | ✅ |
| JSX elements | `quote_codegen.rs` + Ratatui plugin | ✅ |
| JSX fragments | `quote_codegen.rs` + Ratatui plugin | ✅ |
| JSX spread attributes | `quote_codegen.rs` + Ratatui plugin | ✅ |
| Variable declarations | `quote_codegen_stmts.inc` | ✅ |
| If/else | `quote_codegen_stmts.inc` | ✅ |
| For/while/do-while | `quote_codegen_stmts.inc` | ✅ |
| Switch | `quote_codegen_stmts.inc` | ✅ |
| Try/catch/finally | `quote_codegen_stmts.inc` | ✅ |
| Break/continue/return/throw | `quote_codegen_stmts.inc` | ✅ |
| Labeled statements | `quote_codegen_stmts.inc` | ✅ |
| For-in/for-of/for-await-of | `quote_codegen_stmts.inc` | ✅ |
| Class declarations | `quote_codegen.rs` | ✅ |
| Enum declarations | `quote_codegen_stmts.inc` | ✅ |
| Import/export | `quote_codegen_stmts.inc` + bundler | ✅ |
| Destructuring | `quote_codegen_stmts.inc` | ✅ |
| Type erasure (as/satisfies/!) | `quote_codegen_exprs.inc` | ✅ |
| Using/await using | `quote_codegen_stmts.inc` | ✅ |

**Compile-path status:** All P0 and P1 constructs have codegen. Pending tasks in Phases 17–33 extend this to advanced/runtime-specific features, prototype method codegen, and edge-case HIR variants.

---

## Complete Feature Taxonomy

The 407 tasks map to the following exhaustive taxonomy of TS/TSX/React/Ink features. Every leaf feature has a dedicated task and Ink example.

### JavaScript Expressions (Tasks 042–054, 066, 079, 177, 191–192, 236, 294, 345–350, 392–406)
- Literals: number, string, boolean, null, undefined, bigint, regexp, object, array
- Operators: arithmetic, bitwise, logical, comparison, instanceof, in, typeof, delete, void, comma
- Assignment: simple, compound, logical assignment (`||=`, `&&=`, `??=`)
- Update: prefix/postfix increment/decrement
- Member access: dot, computed, optional chaining (`?.`)
- Call expressions: direct, optional call (`?.()`), `new`, `super()`
- `super` member access: `super.method()`, `super[prop]()` (Task 407)
- Functions: arrow, named function expression, anonymous function expression, IIFE
- Async/await, generators (`function*`, `yield`, `yield*`)
- Template literals, tagged templates
- Spread in arrays/objects/calls, rest in params/destructuring
- Type expressions: `as`, `satisfies`, `!` (non-null), type assertions in JSX (Task 347)
- Conditional (ternary), nullish coalescing (`??`), optional chaining in JSX attrs (Task 349)
- Sequence (comma) expressions, `void` expressions

### JavaScript Statements (Tasks 042–045, 170, 177, 178, 203, 237, 238, 298, 388, 391, 392–406)
- Variable declarations: `var`, `let`, `const`, block scoping
- Control flow: `if`/`else`, `switch` (with/without fallthrough), `for`, `while`, `do-while`
- Iteration: `for-in`, `for-of`, `for-await-of`, `break`, `continue`, labeled break/continue
- Exception handling: `try/catch/finally`, `try/finally` without catch, optional catch binding, `throw`
- `return`, `debugger`, labeled statements, `with` (legacy)
- `using` / `await using` (ES2024)

### Declarations & Classes (Tasks 055–057, 083, 093–094, 126–127, 143, 185, 191–192, 200–201, 222–223, 296–297, 393–397)
- Class declarations/expressions: `extends`, `super`, `constructor`, methods, static methods
- Access modifiers: `public`, `private`, `protected`, `readonly`
- `abstract` classes, `implements`, `override`
- Getters/setters (computed and literal), accessor fields (TS 5.0)
- Private fields `#field`, private methods `#method()`, `#field in obj`
- Static blocks, computed class members, class field initializers
- Parameter properties (`constructor(public x: string)`)
- Function declarations, function overloads
- `new.target`

### TypeScript Type System (Tasks 066–067, 082, 087, 097–098, 100–103, 109–110, 115–116, 184, 240, 264, 287–291, 351–360, 377–381, 384, 385–386)
- Type aliases, interfaces, multiple interface extends
- Generics, generic function components (Task 336), generic JSX type args (Task 377)
- `as const`, literal types, tuple types, named tuple members, readonly tuples
- Utility types: `Partial`, `Required`, `Pick`, `Omit`, `Record`, `ReturnType`, `Extract`, `Exclude`, `NonNullable`, `Awaited`, `InstanceType`, `ConstructorParameters`, `ThisType`
- String manipulation types: `Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize`
- Mapped types (with `as` clause, key remapping, recursive readonly)
- Conditional types, distributive conditional, nested conditional, `infer`
- Template literal types, template literal unions
- Discriminated unions, exhaustive checks, branded/opaque types
- User-defined type guards (`is`), `asserts` predicates
- `satisfies` in various contexts, `as` assertions, non-null assertion
- `typeof` on classes, `keyof` on arrays/tuples, `keyof typeof` pattern
- `unknown`, `never`, `this` types, `unique symbol`
- Type erasure: all type-level constructs erased before codegen

### Modules & Imports (Tasks 058–059, 085–086, 119, 138, 141–142, 150, 180, 202, 211–217, 243–244, 282–283, 292–293, 305, 382–383)
- Named/default exports, `export =`, `import = require()`
- Re-exports: `export * from`, `export * as ns from`, `export type { X } from`
- Type-only imports: `import type`, `import { type X }`, inline type imports
- Dynamic imports `import()`, import attributes (`with { type: "json" }`)
- `import.meta.url`, `import.meta.resolve`, `import.meta.env`
- Barrel exports, namespace imports, module-level `render()`
- tsconfig `paths`, `moduleResolution`, `esModuleInterop`, `isolatedModules`
- AMD module directives, project references, package.json exports/imports

### JSX (Tasks 061, 137, 139, 161–163, 196–199, 204, 213, 232–233, 262, 268, 269, 347–350, 377, 398)
- Elements, fragments (`<>...</>`), fragment shorthand
- Spread attributes `{...props}`, boolean attributes, spread children
- Dynamic components, member expressions (`My.Component`), namespaced elements
- Generic type arguments in JSX (`<Component<T> />`)
- Conditional rendering, nullish coalescing in attrs, optional chaining in attrs
- `dangerouslySetInnerHTML`, `key` prop, callback refs, `defaultProps`
- JSX pragmas: `jsxImportSource`, `jsxFactory`, `jsxFragmentFactory`
- Server components (`"use server"` / `"use client"`)

### React Patterns (Tasks 060, 090–091, 106–108, 117, 130, 152–154, 162–163, 193–194, 198–201, 279–281, 330–344, 377–378, 392–406)
- Hooks: `useState`, `useEffect`, `useLayoutEffect`, `useInsertionEffect`, `useCallback`, `useMemo`, `useRef`, `useReducer`, `useContext`, `useId`, `useTransition`, `useDeferredValue`, `useSyncExternalStore`, `useImperativeHandle`, `useDebugValue`, `useFormStatus`, `useOptimistic`, `useActionState`, `use`
- Hook patterns: cleanup functions, dependency arrays, init functions, custom hook composition
- Refs: `forwardRef`, `createRef`, callback refs, generic forwardRef, `displayName`
- Context: `createContext`, `Provider`, `Consumer`, `displayName`, `defaultValue`, multiple providers, context reducer
- `memo`, `lazy`, `Suspense`, `ErrorBoundary`, `Profiler`, `StrictMode`
- `Children` API: `map`, `forEach`, `count`, `only`, `toArray`, `cloneElement`, `isValidElement`
- Component patterns: render props, HOCs, compound components, polymorphic components, generic components, controlled/uncontrolled
- `React.FC`, `PropsWithChildren`, `defaultProps`
- Class lifecycle: `componentDidMount`, `componentWillUnmount`, `getDerivedStateFromError`, `componentDidCatch`

### Ink/TUI Specific (Tasks 062–065, 132–140, 323–325)
- Bridge hooks: `useInput`, `useApp`, `useStdin`, `useStdout`, `useStderr`, `useWindowSize`, `useFocus`, `useFocusManager`, `useCursor`, `useAnimation`
- Bridge components: `Box`, `Text`, `Newline`, `Spacer`, `Static`, `Transform`
- Layout: `measureElement`, `useBoxMetrics`
- `process` global, `setInterval`/`clearInterval`, module-level `render()`

### Runtime APIs — Built-in Objects (Tasks 088–089, 104–105, 111–114, 118, 121–123, 125, 128–129, 155–156, 159, 166–167, 171–176, 179, 181–183, 190, 205, 208–210, 218, 219–220, 225–226, 245, 248, 255–256, 264–265, 273–274, 278, 287, 299, 303, 326–329, 361–375, 392–426)
- **Array:** `from`, `of`, `isArray`, `fromAsync`, `flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `toSpliced`, `with`, `includes`, `findLast`, `findLastIndex`, `reduce`, `reduceRight`, `push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`, `indexOf`, `lastIndexOf`, `every`, `some`, `filter`, `find`, `findIndex`, `map`, `forEach`, `concat`, `join`, `slice`, `fill`, `copyWithin`, `toLocaleString`
- **String:** `charAt`, `charCodeAt`, `codePointAt`, `concat`, `endsWith`, `includes`, `indexOf`, `lastIndexOf`, `localeCompare`, `match`, `matchAll`, `normalize`, `padEnd`, `padStart`, `repeat`, `replace`, `replaceAll`, `search`, `slice`, `split`, `substring`, `startsWith`, `toLowerCase`, `toUpperCase`, `trim`, `trimStart`, `trimEnd`, `at`, `isWellFormed`, `toWellFormed`, `toLocaleString`
- **Number:** `isFinite`, `isNaN`, `isInteger`, `isSafeInteger`, `parseInt`, `parseFloat`, `EPSILON`, `MAX_SAFE_INTEGER`, `MIN_SAFE_INTEGER`, `NaN`, `POSITIVE_INFINITY`, `NEGATIVE_INFINITY`, `toFixed`, `toExponential`, `toPrecision`, `toLocaleString`
- **Math:** `abs`, `acos`, `acosh`, `asin`, `asinh`, `atan`, `atan2`, `atanh`, `cbrt`, `ceil`, `clz32`, `cos`, `cosh`, `exp`, `expm1`, `floor`, `fround`, `hypot`, `imul`, `log`, `log10`, `log1p`, `log2`, `max`, `min`, `pow`, `random`, `round`, `sign`, `sin`, `sinh`, `sqrt`, `tan`, `tanh`, `trunc`, `E`, `LN10`, `LN2`, `LOG10E`, `LOG2E`, `PI`, `SQRT1_2`, `SQRT2`
- **Date:** `now`, `parse`, `UTC`, getters (`getTime`, `getFullYear`, `getMonth`, `getDate`, `getHours`, `getMinutes`, `getSeconds`, `getMilliseconds`), setters (`setTime`, `setFullYear`, `setMonth`, `setDate`, `setHours`), `toISOString`, `toUTCString`, `toDateString`, `toTimeString`, `toLocaleDateString`, `toLocaleTimeString`, `toJSON`, `valueOf`, `[Symbol.toPrimitive]`
- **RegExp:** `test`, `exec`, `compile`, `toString`, flags (`g`, `i`, `m`, `u`, `y`, `d`, `s`, `v`), named capture groups, `matchAll`
- **Object:** `create`, `defineProperty`, `defineProperties`, `freeze`, `seal`, `assign`, `entries`, `values`, `keys`, `hasOwn`, `getOwnPropertyDescriptors`, `getOwnPropertyDescriptor`, `getOwnPropertyNames`, `getOwnPropertySymbols`, `preventExtensions`, `isExtensible`, `isSealed`, `isFrozen`, `is`, `getPrototypeOf`, `setPrototypeOf`, `groupBy`, `fromEntries`, `prototype.toString`, `prototype.toLocaleString`, `prototype.isPrototypeOf`, `prototype.propertyIsEnumerable`, `prototype.valueOf`, `prototype.hasOwnProperty`
- **Function:** `prototype.bind`, `prototype.call`, `prototype.apply`, `prototype.length`, `prototype.name`, `prototype.prototype`, `prototype[Symbol.hasInstance]`
- **JSON:** `stringify` (with replacer), `parse`
- **Promise:** `resolve`, `reject`, `all`, `race`, `allSettled`, `any`, `withResolvers`, `prototype.then`, `prototype.catch`, `prototype.finally`
- **Map/Set/WeakMap/WeakSet:** `set`, `get`, `has`, `delete`, `clear`, `size`, `keys`, `values`, `entries`, `forEach`, `add`, `groupBy` (Map), ES2025 Set methods (`intersection`, `union`, `difference`, `symmetricDifference`, `isSubsetOf`, `isSupersetOf`, `isDisjointFrom`)
- **Symbol:** `for`, `keyFor`, `iterator`, `asyncIterator`, `hasInstance`, `toStringTag`, `toPrimitive`, `species`, `isConcatSpreadable`, `unscopables`, `dispose`, `asyncDispose`, `match`, `replace`, `search`, `split`
- **WeakRef/FinalizationRegistry:** `WeakRef.prototype.deref`, `FinalizationRegistry.prototype.register`
- **Proxy/Reflect:** `Proxy` constructor with handlers, `Reflect.apply`, `Reflect.construct`, `Reflect.defineProperty`, `Reflect.deleteProperty`, `Reflect.get`, `Reflect.getOwnPropertyDescriptor`, `Reflect.getPrototypeOf`, `Reflect.has`, `Reflect.isExtensible`, `Reflect.ownKeys`, `Reflect.preventExtensions`, `Reflect.set`, `Reflect.setPrototypeOf`
- **ArrayBuffer/TypedArrays/DataView:** `ArrayBuffer`, `Uint8Array`, `Int8Array`, `Uint16Array`, `Int16Array`, `Uint32Array`, `Int32Array`, `Float32Array`, `Float64Array`, `BigInt64Array`, `BigUint64Array`, `DataView`
- **BigInt:** literals, operations, `prototype.toString`, `prototype.valueOf`, `BigInt.asIntN`, `BigInt.asUintN`
- **Error:** `Error`, `TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`, `AggregateError`, `SuppressedError`, `cause`, `captureStackTrace`, `stackTraceLimit`
- **Generator/Iterator:** `function*`, `yield`, `yield*`, `Generator.prototype.return`, `Generator.prototype.throw`, `Iterator.from`, `Iterator.prototype.map/filter/take/drop/flatMap/reduce/toArray`, `AsyncIterator` helpers
- **console:** `log`, `error`, `warn`, `info`, `debug`, `dir`, `assert`, `count`, `countReset`, `group`, `groupEnd`, `trace`, `time`, `timeLog`, `timeEnd`, `table`
- **globalThis:** `globalThis`, `global`, `window`, `self`, `undefined`, `Infinity`, `NaN`
- **eval/legacy:** `eval()`, `with`, `escape`, `unescape`

### Runtime APIs — Web & Node.js (Tasks 133–136, 218, 229, 249–250, 251–253, 254, 306–308, 309–310, 311–314, 315–322, 324–325, 370–375, 392–426)
- **Timers:** `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval`, `setImmediate`, `clearImmediate`, `requestAnimationFrame`, `cancelAnimationFrame`, `queueMicrotask`
- **Encoding:** `TextEncoder`, `TextDecoder`, `atob`, `btoa`
- **URL:** `URL`, `URLSearchParams`, `URLPattern`
- **Web Crypto:** `crypto.randomUUID`, `crypto.getRandomValues`, `crypto.subtle.digest`
- **Streams:** `ReadableStream`, `WritableStream`, `TransformStream`, `CompressionStream`, `DecompressionStream`
- **Network:** `fetch`, `Headers`, `Request`, `Response`, `WebSocket`
- **Storage:** `localStorage` / `sessionStorage` (`getItem`, `setItem`, `removeItem`, `clear`, `length`, `key`)
- **Events:** `EventTarget`, `CustomEvent`, `AbortController`, `AbortSignal`, `EventEmitter`
- **File/Blob:** `Blob`, `File`, `FileReader`, `FormData`
- **Performance:** `performance.now`, `performance.mark`, `performance.measure`, `performance.timing`, `performance.navigation`, `PerformanceObserver`
- **Intl:** `DateTimeFormat`, `NumberFormat`, `ListFormat`, `RelativeTimeFormat`, `PluralRules`, `Collator`, `Segmenter`, `DisplayNames`, `Locale`
- **Structured Clone:** `structuredClone`
- **Atomics/SharedArrayBuffer:** `Atomics.add`, `Atomics.load`, `Atomics.store`, `Atomics.compareExchange`, `Atomics.wait`, `Atomics.notify`, `SharedArrayBuffer`
- **WebAssembly:** `WebAssembly.instantiate`, `WebAssembly.Module`, `WebAssembly.Memory`, `WebAssembly.Table`
- **Process:** `process.exit`, `process.env`, `process.stdin`, `process.stdout`, `process.pid`, `process.cwd`, `process.uptime`, `process.hrtime`, `process.memoryUsage`, `process.platform`, `process.version`, `process.versions`, `process.arch`, `process.release`, `process.nextTick`
- **Node.js modules:** `fs`, `fs/promises`, `crypto`, `util`, `stream`, `stream/web`, `readline`, `assert`, `child_process`, `http`/`https`, `path`, `os`, `timers/promises`, `Buffer`
- **Browser globals:** `window`, `document`, `navigator` (`userAgent`, `language`, `onLine`, `platform`), `location`, `history`
- **Temporal API:** `Temporal.Now`, `Temporal.Instant`, `Temporal.PlainDate`, `Temporal.PlainTime`, `Temporal.Duration`

### Compiler Options & Directives (Tasks 197, 214–217, 230–231, 242–244, 257–261, 265–268, 282–285, 292–293)
- Directives: `// @ts-expect-error`, `// @ts-ignore`, `// @ts-nocheck`, `// @ts-check`, `/// <reference types="..." />`, `/// <reference path="..." />`, `/// <amd-module name="..." />`
- Compiler options: `strict`, `strictNullChecks`, `noImplicitAny`, `noImplicitReturns`, `noFallthroughCasesInSwitch`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `preserveValueImports`, `verbatimModuleSyntax`, `isolatedModules`, `resolveJsonModule`, `esModuleInterop`, `allowSyntheticDefaultImports`, `moduleResolution`, `paths`, `baseUrl`, `preserveConstEnums`, `downlevelIteration`, `jsxPreserve`, `emitDecoratorMetadata`
- Module formats: ESM, CJS interop, AMD references, barrel exports, type-only imports/exports

---

## Coverage Strategy

### Philosophy
Every practical TS/TSX/React/Ink feature is exercised by a real Ink TUI example. The example must call the **real API directly** — no fallbacks, no polyfills, no `typeof X !== 'undefined'` guards. If an API is missing in rquickjs or the compile path, the parity harness fails with 100% mismatch, and the bridge/runtime/codegen gets fixed.

### Three validation layers
1. **Parser → HIR:** `cargo test --bin runts` validates that oxc_parser produces correct HIR shapes.
2. **Dev path (rquickjs):** `scripts/parity.sh --env rq` validates that TSX→JS→rquickjs produces output identical to deno.
3. **Compile path:** `scripts/parity.sh --env compile` validates that TSX→HIR→Rust codegen→binary produces output identical to deno.

### Coverage gaps
The authoritative list of remaining features is maintained in `tasks/index.json`. As tasks are completed and their examples pass parity in all 3 environments, the corresponding gaps are removed. The goal is zero gaps by Task 426.

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
| **Do not simplify examples with fallbacks or polyfills.** | If an API is missing, the parity harness must fail with 100% mismatch. Fix the bridge/runtime, not the example. Examples must call the real API. |

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

### Infrastructure ✅
- [x] `cargo build` passes with 0 errors, 0 warnings.
- [x] `cargo test --test rq_parity` passes ≥90% of examples.
- [x] `cargo test --bin runts` exits 0.
- [x] `cargo test --test compile_codegen` passes.
- [x] Zero commented-out test modules.
- [x] No file > 500 lines, no fn > 40 lines, no complexity > 10.
- [x] No references to HIR interpreter, Taffy, or `render_tsx`.
- [x] Docs accurately describe rquickjs + Yoga architecture.

### Completed Phases ✅ (176 tasks)
- [x] Phase 0–5: Build, engine, compile path, coverage, cleanup, hardening (37 tasks).
- [x] Phase 6: 27 Ink examples covering core TS/TSX/React/Ink features (Tasks 041–067).
- [x] Phase 7: HIR expansion — optional chaining, enums, private fields, generators, dynamic import, decorators (Tasks 068–074).
- [x] Phase 8: Compile-path integration tests (Tasks 075–077).
- [x] Phase 9: Final audit and coverage matrix (Task 078).
- [x] Phase 10: 21 Ink examples for extended TS/TSX coverage (Tasks 079–099).
- [x] Phase 11: 21 Ink examples for type system + runtime API deep coverage (Tasks 100–120).
- [x] Phase 12: 11 Ink examples for runtime API + type system completion (Tasks 121–131).
- [x] Phase 13: Real-world validation — `../tui1` port with all bridge gaps fixed (Tasks 132–140).
- [x] Phase 14: 8 Ink examples for final runtime API + module pattern completion (Tasks 141–148).
- [x] Phase 15: 15 Ink examples for advanced React patterns + deep runtime coverage (Tasks 149–163).
- [x] Phase 16: 15 Ink examples for operator + syntax + runtime API completion (Tasks 164–178).
- [x] Phase 17: Partial — ES2024 `using`, import attributes, `Array.fromAsync`, `Promise.withResolvers`, `Object.groupBy`, `const` type params (Tasks 179–184 complete).

### Pending Phases ⏳ (231 tasks)
- [ ] Phase 17: Remaining ES2024 + TypeScript 5.0+ features (Tasks 185–190).
- [ ] Phase 18: Expression-level + React pattern + runtime API completion (Tasks 191–205).
- [ ] Phase 19: Tagged templates + compiler options + Reflect deep coverage + ES2025 (Tasks 206–220).
- [ ] Phase 20: Advanced language + runtime + React patterns + compile infrastructure (Tasks 221–235).
- [ ] Phase 21: Niche language features + TypeScript config + Web Streams + Crypto (Tasks 236–250).
- [ ] Phase 22: Web APIs + TypeScript config edge cases + JSX/TS language patterns (Tasks 251–270).
- [ ] Phase 23: Emerging runtime APIs + React patterns + module resolution + compile infrastructure (Tasks 271–285).
- [ ] Phase 24: Orphaned example audit + advanced type system + language edge cases (Tasks 286–299).
- [ ] Phase 25: Advanced destructuring + Node.js/Web runtime APIs (Tasks 300–314).
- [ ] Phase 26: Node.js standard library + browser globals + binary data + Promise/Math completion (Tasks 315–329).
- [ ] Phase 27: React hook patterns + JSX expression patterns + component composition (Tasks 330–350).
- [ ] Phase 28: Advanced TypeScript type system patterns (Tasks 351–360).
- [ ] Phase 29: Array/Object/String/Number/Date/Console/Error/Symbol/Process/Stream/WebAssembly API completion (Tasks 361–375).
- [ ] Phase 30: Final comprehensive coverage audit (Task 376), updated for 407 tasks.
- [ ] Phase 31: Advanced JSX + React edge cases (Tasks 377–391).
- [ ] Phase 32: Core language + React + runtime edge cases (Tasks 392–406).
- [ ] Phase 33: HIR & codegen edge cases (Tasks 407–426).
- [ ] `scripts/parity.sh --env all` passes all examples with 100% match in all 3 environments.
