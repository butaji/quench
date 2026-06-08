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
| 092 | `ink-namespace-declare` | `namespace`, `declare` | P2 ✅ |
| 093 | `ink-override-implements` | `override`, `implements` | P2 |
| 094 | `ink-abstract-class` | `abstract` classes | P2 |
| 095 | `ink-new-target` | `new.target` | P3 |
| 096 | `ink-reflect-api` | `Reflect` API | P3 |
| 097 | `ink-template-literal-types` | Template literal types | P3 |
| 098 | `ink-infer-conditional` | `infer` in conditional types | P3 |
| 099 | `ink-regexp-advanced` | RegExp flags, `matchAll` | P3 |

**Tasks:** 079–099 | **Status:** 🔄 Pending (19 tasks, 2 completed)

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
| 108 | `ink-context-advanced` | Context `displayName`, `defaultValue`, multiple providers | P1 ✅ |
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

### Phase 12: Runtime API + Type System Completion
**Goal:** Final practical TS/TSX/React/Ink features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 121 | `ink-json-api` | `JSON.stringify`, `JSON.parse` | P1 |
| 122 | `ink-function-bind` | `bind`, `call`, `apply` | P1 |
| 123 | `ink-object-meta` | `create`, `defineProperty`, `freeze`, `seal`, `assign` | P1 |
| 124 | `ink-queue-microtask` | `queueMicrotask` | P1 |
| 125 | `ink-error-cause` | `Error` with `cause` | P1 |
| 126 | `ink-static-block` | Class `static {}` blocks | P1 |
| 127 | `ink-private-methods` | Private methods `#method()`, `#field in obj` | P1 |
| 128 | `ink-number-static` | `Number.isFinite`, `isNaN`, `parseInt`, `parseFloat`, `EPSILON` | P1 |
| 129 | `ink-array-immutable` | `toSpliced`, `with` (ES2023) | P1 |
| 130 | `ink-react-refs-debug` | `createRef`, `useDebugValue` | P2 |
| 131 | `ink-function-overloads` | Function overloads | P2 |

**Tasks:** 121–131 | **Status:** 🔄 Pending (11 tasks)

---

### Phase 13: Real-World Validation (`../tui1` Port)
**Goal:** The `../tui1` real-world Ink chat example compiles and renders identically in all 3 environments with 100% output match.

| Task | Gap | Priority |
|------|-----|----------|
| 132 | Port `../tui1` — comprehensive feature audit | P0 |
| 133 | `process` global (`process.exit`, `process.env`, `process.stdin`, `process.stdout`) | P0 |
| 134 | `setInterval` / `clearInterval` | P0 |
| 135 | `Date` object (`new Date`, `Date.now`, `toLocaleTimeString`) | P0 |
| 136 | `Array.prototype.splice` | P1 |
| 137 | React Fragment shorthand `<></>` | P0 |
| 138 | Dynamic import of node built-ins (`import("node:readline")`) | P0 |
| 139 | `/** @jsxImportSource react */` pragma | P1 |
| 140 | Module-level `render()` call | P1 |

**Tasks:** 132–140 | **Status:** 🔄 Pending (9 tasks)

---

### Phase 14: Final Runtime API + Module Pattern Completion
**Goal:** Remaining practical TS/TSX/React/Ink features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 141 | `ink-namespace-reexport` | `export * as ns from "mod"` | P1 |
| 142 | `ink-inline-type-import` | `import { type X }`, `import type * as ns` | P1 |
| 143 | `ink-parameter-properties` | `constructor(public x: string)` | P1 |
| 144 | `ink-console-methods` | `console.log`, `error`, `warn`, `info`, `time`, `timeEnd` | P1 |
| 145 | `ink-uri-encoding` | `encodeURI`, `decodeURI`, `encodeURIComponent`, `decodeURIComponent` | P1 |
| 146 | `ink-array-reduce` | `reduce`, `reduceRight` | P1 |
| 147 | `ink-string-search` | `startsWith`, `endsWith`, `includes`, `repeat` | P1 |
| 148 | `ink-error-subclasses` | `TypeError`, `RangeError`, `ReferenceError` | P1 |

**Tasks:** 141–148 | **Status:** 🔄 Pending (8 tasks)

---

### Phase 15: Advanced React Patterns + Deep Runtime Coverage
**Goal:** Advanced React patterns (render props, HOCs, key/refs) and remaining runtime APIs (Symbol.iterator, ArrayBuffer, performance, structuredClone, arguments, RegExp named groups, string well-formed) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 149 | `ink-asserts-predicate` | `asserts` type predicate | P1 |
| 150 | `ink-import-types` | `type T = import("./mod").Type` | P1 |
| 151 | `ink-reference-directive` | `/// <reference types="..." />` | P2 |
| 152 | `ink-use-insertion-effect` | `useInsertionEffect` | P1 |
| 153 | `ink-render-props` | Render props pattern | P1 |
| 154 | `ink-hoc` | Higher-order components | P1 |
| 155 | `ink-symbol-iterator` | `Symbol.iterator`, custom iterables | P1 |
| 156 | `ink-arraybuffer` | `ArrayBuffer`, `Uint8Array`, `DataView` | P2 |
| 157 | `ink-performance` | `performance.now()`, `performance.mark` | P2 |
| 158 | `ink-structured-clone` | `structuredClone` | P2 |
| 159 | `ink-arguments` | `arguments` object | P2 |
| 160 | `ink-regexp-named-groups` | Named capture groups | P2 |
| 161 | `ink-string-wellformed` | `isWellFormed`, `toWellFormed` | P2 |
| 162 | `ink-key-prop` | `key` prop in lists and fragments | P1 |
| 163 | `ink-ref-callback` | Callback refs, `useRef` with initial value | P1 |

**Tasks:** 149–163 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 16: Operator + Syntax + Runtime API Completion
**Goal:** Final JavaScript operators, syntax features, and runtime APIs exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 164 | `ink-in-operator` | `in` operator (`prop in obj`) | P1 |
| 165 | `ink-for-await-of` | `for await...of` async iteration | P1 |
| 166 | `ink-object-entries-values` | `Object.entries`, `Object.values`, `Object.keys` | P1 |
| 167 | `ink-array-static` | `Array.from`, `Array.of`, `Array.isArray` | P1 |
| 168 | `ink-parse-global` | `parseInt`, `parseFloat`, `isNaN`, `isFinite` | P1 |
| 169 | `ink-import-meta-url` | `import.meta.url` | P1 |
| 170 | `ink-debugger-labeled` | `debugger`, labeled statements | P2 |
| 171 | `ink-number-string-proto` | `Number.prototype.toFixed`, `String.prototype.charAt` etc | P1 |
| 172 | `ink-array-mutators` | `push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse` | P1 |
| 173 | `ink-array-searchers` | `indexOf`, `lastIndexOf`, `every`, `some`, `filter`, `find`, `findIndex` | P1 |
| 174 | `ink-date-comprehensive` | `getTime`, `getFullYear`, `toISOString`, `toUTCString` | P1 |
| 175 | `ink-regexp-test-exec` | `RegExp.prototype.test`, `exec` | P1 |
| 176 | `ink-promise-resolve-reject` | `Promise.resolve`, `Promise.reject` | P1 |
| 177 | `ink-void-comma-increment` | `void`, comma operator, `++`/`--` | P1 |
| 178 | `ink-spread-call-destructure` | Spread in calls, destructuring in params/catch/for-of | P1 |

**Tasks:** 164–178 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 17: ES2024 + TypeScript 5.0+ Feature Completion
**Goal:** Latest ECMAScript 2024 and TypeScript 5.0+ features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 179 | `ink-using-declaration` | `using` / `await using` (ES2024) | P1 |
| 180 | `ink-import-attributes` | Import attributes (`with { type: "json" }`) | P1 |
| 181 | `ink-array-from-async` | `Array.fromAsync` | P1 |
| 182 | `ink-promise-with-resolvers` | `Promise.withResolvers` | P1 |
| 183 | `ink-object-group-by` | `Object.groupBy` / `Map.groupBy` | P1 |
| 184 | `ink-const-type-param` | `const T` type parameters (TS 5.0) | P1 |
| 185 | `ink-accessor-field` | `accessor` class fields (TS 5.0) | P1 |
| 186 | `ink-symbol-async-iterator` | `Symbol.asyncIterator` | P1 |
| 187 | `ink-throw-expression` | Throw expressions (Stage 3) | P2 |
| 188 | `ink-type-annotation-catch` | Type annotation in `catch` clause | P1 |
| 189 | `ink-satisfies-expr` | `satisfies` in object/array literals | P1 |
| 190 | `ink-iterator-helpers` | Iterator helpers (`map`, `filter`, `take`) | P2 |

**Tasks:** 179–190 | **Status:** 🔄 Pending (12 tasks)

---

### Phase 18: Expression-Level + React Pattern + Runtime API Completion
**Goal:** Final TS/TSX expression-level features, React type patterns, advanced runtime APIs, JSX edge cases, and legacy features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 191 | `ink-class-expression` | Anonymous class expressions | P1 |
| 192 | `ink-function-expression` | Anonymous/named function expressions | P1 |
| 193 | `ink-react-fc-type` | `React.FC`, `React.FunctionComponent` | P1 |
| 194 | `ink-props-with-children` | `React.PropsWithChildren` | P1 |
| 195 | `ink-string-advanced` | `localeCompare`, `normalize`, `codePointAt`, `fromCodePoint`, `concat`, `charAt`, `charCodeAt` | P2 |
| 196 | `ink-array-advanced` | `findLast`, `findLastIndex`, `fill`, `copyWithin`, `at` | P2 |
| 197 | `ink-ts-directive` | `// @ts-expect-error`, `// @ts-ignore` | P2 |
| 198 | `ink-dangerously-set-inner-html` | `dangerouslySetInnerHTML` JSX prop | P2 |
| 199 | `ink-default-props` | `defaultProps` on function components | P2 |
| 200 | `ink-class-lifecycle` | `componentDidMount`, `componentWillUnmount` | P1 |
| 201 | `ink-get-derived-state-from-error` | `static getDerivedStateFromError`, `componentDidCatch` | P1 |
| 202 | `ink-export-star-as` | `export * as ns from "mod"` | P1 |
| 203 | `ink-with-statement` | `with` statement (legacy) | P3 |
| 204 | `ink-jsx-spread-attribute` | JSX spread attributes `{...props}` | P1 |
| 205 | `ink-set-prototype` | `Set`/`Map`/`WeakMap`/`WeakSet` prototype methods | P2 |

**Tasks:** 191–205 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 19: Tagged Templates + Compiler Options + Reflect Deep Coverage + ES2025
**Goal:** Advanced expression patterns, TypeScript compiler options, deep runtime API coverage, and ES2025 features exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 206 | `ink-tagged-template` | Tagged template literals | P1 |
| 207 | `ink-function-constructor` | `Function` constructor | P2 |
| 208 | `ink-object-prototype` | Object prototype methods (`toString`, `valueOf`, `hasOwnProperty`, etc.) | P2 |
| 209 | `ink-object-static` | Object static methods (`is`, `setPrototypeOf`, `getPrototypeOf`, etc.) | P2 |
| 210 | `ink-reflect-deep` | Reflect methods (`apply`, `construct`, `defineProperty`, etc.) | P2 |
| 211 | `ink-tsconfig-paths` | tsconfig `paths` mapping | P1 |
| 212 | `ink-esmodule-interop` | `esModuleInterop`, `allowSyntheticDefaultImports` | P1 |
| 213 | `ink-jsx-factory-pragma` | `jsxFactory`, `jsxFragmentFactory`, `jsxImportSource` pragmas | P1 |
| 214 | `ink-strict-mode` | TypeScript strict mode flags | P1 |
| 215 | `ink-downlevel-iteration` | `downlevelIteration` for ES5 targets | P2 |
| 216 | `ink-preserve-const-enum` | `preserveConstEnums` behavior | P2 |
| 217 | `ink-module-resolution` | `moduleResolution` modes (`node`, `bundler`, `classic`) | P1 |
| 218 | `ink-web-api` | Web APIs (`URL`, `TextEncoder`, `Headers`, etc.) | P2 |
| 219 | `ink-regexp-escape` | `RegExp.escape` (ES2025) | P3 |
| 220 | `ink-set-es2025` | Set methods (`intersection`, `union`, `isSubsetOf`, etc.) | P3 |

**Tasks:** 206–220 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 20: Advanced Language + Runtime + React Patterns + Compile Infrastructure
**Goal:** Advanced language features (eval, super calls, private-in, import.meta.resolve, globalThis, BigInt), runtime API deep coverage (Intl, Atomics), compile path infrastructure (source maps, verbatimModuleSyntax), JSX advanced patterns (member expressions, namespaces), and React patterns (startTransition, React 19 hooks) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 221 | `ink-eval` | `eval()` function | P2 |
| 222 | `ink-super-call` | `super()` in class constructors | P1 |
| 223 | `ink-private-in` | `#field in obj` (private identifier in `in`) | P1 |
| 224 | `ink-import-meta-resolve` | `import.meta.resolve` | P1 |
| 225 | `ink-global-this` | `globalThis` | P1 |
| 226 | `ink-bigint-ops` | BigInt literals and operations | P1 |
| 227 | `ink-intl-datetime` | `Intl.DateTimeFormat` | P2 |
| 228 | `ink-intl-number` | `Intl.NumberFormat` | P2 |
| 229 | `ink-atomics` | `Atomics` API | P3 |
| 230 | `ink-source-map` | Source map generation | P2 |
| 231 | `ink-verbatim-module-syntax` | `verbatimModuleSyntax` (TS 5.0) | P1 |
| 232 | `ink-jsx-member` | JSX member expressions (`My.Component`) | P1 |
| 233 | `ink-jsx-namespace` | JSX namespaced elements (`ns:tag`) | P2 |
| 234 | `ink-start-transition` | `startTransition` | P2 |
| 235 | `ink-react-19-hooks` | `useFormStatus`, `useOptimistic`, `useActionState`, `use` | P3 |

**Tasks:** 221–235 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 21: Niche Language Features + TypeScript Config + Web Streams + Crypto
**Goal:** Niche language features (sequence expressions, labeled break/continue, nested/default destructuring, readonly tuples, keyof typeof), TypeScript configuration (reference path, isolatedModules, resolveJsonModule), and runtime APIs (AggregateError, URLSearchParams, TextDecoder, Web Crypto, ReadableStream, CompressionStream) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 236 | `ink-sequence-expression` | Comma operator sequences | P2 |
| 237 | `ink-labeled-break-continue` | Labeled `break` and `continue` | P2 |
| 238 | `ink-nested-destructure` | Nested object/array destructuring | P1 |
| 239 | `ink-default-destructure` | Default values in destructuring | P1 |
| 240 | `ink-readonly-tuple` | Readonly tuples and arrays | P1 |
| 241 | `ink-keyof-typeof` | `keyof typeof` pattern | P1 |
| 242 | `ink-reference-path` | `/// <reference path="..." />` | P2 |
| 243 | `ink-isolated-modules` | `isolatedModules` compiler option | P1 |
| 244 | `ink-resolve-json-module` | `resolveJsonModule` | P1 |
| 245 | `ink-aggregate-error` | `AggregateError` | P2 |
| 246 | `ink-url-search-params` | `URLSearchParams` | P2 |
| 247 | `ink-text-decoder` | `TextDecoder` | P2 |
| 248 | `ink-crypto-random` | `crypto.randomUUID` | P2 |
| 249 | `ink-readable-stream` | `ReadableStream` | P3 |
| 250 | `ink-compression-stream` | `CompressionStream` / `DecompressionStream` | P3 |

**Tasks:** 236–250 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 22: Web APIs + TypeScript Config Edge Cases + JSX/TS Language Patterns
**Goal:** Web APIs (WebSocket, FormData, Blob, EventTarget, FinalizationRegistry, SharedArrayBuffer), TypeScript configuration edge cases (noImplicitReturns, noFallthroughCasesInSwitch, noUncheckedIndexedAccess, exactOptionalPropertyTypes, preserveValueImports, jsxPreserve, emitDecoratorMetadata), and advanced TS/JSX language patterns (JSX spread children, rest destructuring, named tuples, const enum, ambient modules, ts-nocheck, server components) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 251 | `ink-websocket` | WebSocket API | P2 |
| 252 | `ink-form-data` | FormData API | P2 |
| 253 | `ink-blob` | Blob and FileReader APIs | P2 |
| 254 | `ink-event-target` | EventTarget and CustomEvent | P2 |
| 255 | `ink-finalization-registry` | FinalizationRegistry | P2 |
| 256 | `ink-shared-array-buffer` | SharedArrayBuffer | P3 |
| 257 | `ink-no-implicit-returns` | `noImplicitReturns` compiler option | P2 |
| 258 | `ink-no-fallthrough` | `noFallthroughCasesInSwitch` compiler option | P2 |
| 259 | `ink-no-unchecked-indexed` | `noUncheckedIndexedAccess` compiler option | P2 |
| 260 | `ink-exact-optional` | `exactOptionalPropertyTypes` compiler option | P2 |
| 261 | `ink-preserve-value-imports` | `preserveValueImports` compiler option | P2 |
| 262 | `ink-jsx-spread-children` | Spread in JSX children | P1 |
| 263 | `ink-rest-destructure` | Rest elements in object/array destructuring | P1 |
| 264 | `ink-named-tuple` | Named tuple members | P1 |
| 265 | `ink-const-enum` | `const enum` declaration | P1 |
| 266 | `ink-ambient-module` | Ambient module declarations | P2 |
| 267 | `ink-ts-nocheck` | `// @ts-nocheck` and `// @ts-check` directives | P2 |
| 268 | `ink-jsx-preserve` | JSX `preserve` transform | P2 |
| 269 | `ink-server-component` | `"use server"` / `"use client"` directives | P3 |
| 270 | `ink-decorator-metadata` | `emitDecoratorMetadata` | P3 |

**Tasks:** 251–270 | **Status:** 🔄 Pending (20 tasks)

---

### Phase 23: Emerging Runtime APIs + React Patterns + Module Resolution + Compile Infrastructure
**Goal:** Emerging runtime APIs (Temporal, additional Intl formatters, SuppressedError, String.raw, AsyncGenerator, Iterator.from, DisposableStack, escape/unescape), React patterns (Profiler, StrictMode, createPortal), module resolution (package.json exports/imports, project references), and compile infrastructure (watch mode, declaration files) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 271 | `ink-temporal-api` | Temporal API | P3 |
| 272 | `ink-intl-list` | `Intl.ListFormat`, `RelativeTimeFormat`, `PluralRules`, `Collator` | P3 |
| 273 | `ink-suppressed-error` | `SuppressedError`, `Error.isError` | P3 |
| 274 | `ink-string-raw` | `String.raw` | P2 |
| 275 | `ink-async-generator` | Async generators | P1 |
| 276 | `ink-iterator-from` | `Iterator.from` | P3 |
| 277 | `ink-disposable-stack` | `DisposableStack`, `AsyncDisposableStack` | P3 |
| 278 | `ink-escape-unescape` | Legacy `escape()`, `unescape()` | P3 |
| 279 | `ink-react-profiler` | `React.Profiler` | P2 |
| 280 | `ink-react-strict-mode` | `React.StrictMode` | P2 |
| 281 | `ink-react-portal` | `ReactDOM.createPortal` | P3 |
| 282 | `ink-package-exports` | `package.json` `exports`/`imports` | P2 |
| 283 | `ink-project-references` | TypeScript project references | P2 |
| 284 | `ink-watch-mode` | Watch mode / incremental compilation | P2 |
| 285 | `ink-declaration-files` | `.d.ts` declaration files | P2 |

**Tasks:** 271–285 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 24: Orphaned Example Audit + Advanced Type System + Language Edge Cases
**Goal:** Comprehensive audit of 92 orphaned existing Ink examples, plus advanced type system features (recursive types, distributive conditional types, variadic tuples, branded types), language edge cases (generator return/throw, optional catch binding, nullish in objects, object shorthand, computed class members, class field initializers), and module/type directives (type-only import assertions, AMD module, satisfies on functions) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 286 | — | Audit and track 92 orphaned existing Ink examples | P0 |
| 287 | `ink-recursive-types` | Recursive type aliases/interfaces | P2 |
| 288 | `ink-distributive-conditional` | Distributive conditional types | P2 |
| 289 | `ink-variadic-tuple` | Variadic tuple types | P2 |
| 290 | `ink-generator-return-throw` | `generator.return()` / `generator.throw()` | P2 |
| 291 | `ink-branded-types` | Branded / opaque types | P2 |
| 292 | `ink-type-only-import-assertion` | `import type` with import attributes | P2 |
| 293 | `ink-amd-module` | `/// <reference amd-module name="..." />` | P3 |
| 294 | `ink-satisfies-function` | `satisfies` on function expressions | P2 |
| 295 | `ink-object-pattern-shorthand` | Shorthand properties and method syntax | P1 |
| 296 | `ink-computed-class-members` | Computed property keys in classes | P1 |
| 297 | `ink-class-fields-init` | Class fields with complex initializers | P1 |
| 298 | `ink-optional-catch-binding` | Optional catch binding | P1 |
| 299 | `ink-nullish-in-object` | Nullish values in object literals | P2 |

**Tasks:** 286–299 | **Status:** 🔄 Pending (14 tasks)

---

### Phase 25: Advanced Destructuring + Node.js/Web Runtime APIs
**Goal:** Advanced destructuring patterns (mixed array/object, computed keys, renaming), Node.js runtime APIs (global, Buffer, __dirname, __filename, CommonJS interop, process.nextTick, EventEmitter, path, os), and Web APIs (AbortSignal, fetch, PerformanceObserver, setImmediate, URLPattern) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 300 | `ink-mixed-destructure` | Mixed array/object destructuring | P1 |
| 301 | `ink-computed-destructure` | Computed keys and renaming in destructuring | P1 |
| 302 | `ink-global-object` | `global` object | P1 |
| 303 | `ink-buffer` | Node.js `Buffer` | P1 |
| 304 | `ink-dirname-filename` | `__dirname` and `__filename` | P1 |
| 305 | `ink-commonjs-interop` | `module.exports`, `exports`, `require` | P1 |
| 306 | `ink-abort-signal` | `AbortSignal` and `AbortController` | P1 |
| 307 | `ink-fetch-api` | `fetch`, `Response`, `Request` | P1 |
| 308 | `ink-performance-observer` | `PerformanceObserver`, `mark`, `measure` | P2 |
| 309 | `ink-set-immediate` | `setImmediate` / `clearImmediate` | P2 |
| 310 | `ink-process-nexttick` | `process.nextTick` | P2 |
| 311 | `ink-events-emitter` | `EventEmitter` | P2 |
| 312 | `ink-path-module` | `path` module | P2 |
| 313 | `ink-os-module` | `os` module | P2 |
| 314 | `ink-urlpattern` | `URLPattern` | P3 |

**Tasks:** 300–314 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 26: Node.js Standard Library + Browser Globals + Binary Data + Promise/Math Completion
**Goal:** Node.js standard library modules (fs, crypto, util, stream, readline, assert, child_process, http), process properties (pid, cwd, uptime), browser globals (window, document, navigator, localStorage), binary data APIs (typed arrays, DataView, File), Promise chaining (then/catch/finally), and advanced Math methods/constants exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 315 | `ink-node-fs` | Node.js `fs` module | P1 |
| 316 | `ink-node-crypto` | Node.js `crypto` module | P1 |
| 317 | `ink-node-util` | Node.js `util` module | P1 |
| 318 | `ink-node-stream` | Node.js `stream` module | P1 |
| 319 | `ink-node-readline` | Node.js `readline` module | P1 |
| 320 | `ink-node-assert` | Node.js `assert` module | P2 |
| 321 | `ink-node-child-process` | Node.js `child_process` module | P2 |
| 322 | `ink-node-http` | Node.js `http`/`https` module | P2 |
| 323 | `ink-process-props` | `process` properties (pid, cwd, uptime, hrtime, memoryUsage) | P1 |
| 324 | `ink-browser-globals` | `window`, `document`, `navigator`, `location` | P2 |
| 325 | `ink-web-storage` | `localStorage` / `sessionStorage` | P2 |
| 326 | `ink-typed-arrays` | Typed arrays and `DataView` | P1 |
| 327 | `ink-file-constructor` | `File` constructor | P2 |
| 328 | `ink-promise-chain` | `Promise.prototype.then/catch/finally` + `Promise.any`/`race` | P1 |
| 329 | `ink-math-advanced` | Advanced `Math` methods and constants | P2 |

**Tasks:** 315–329 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 27: React Hook Patterns + JSX Expression Patterns + Component Composition
**Goal:** React hook patterns (useEffect cleanup, useCallback/useMemo dependencies, custom hook composition), React children API (Children.only, Children.toArray), component composition patterns (generic components, polymorphic components, compound components, context reducer, controlled/uncontrolled, forwardRef with generics, render props with generics, HOCs with generics), and JSX expression patterns (optional call, non-null assertion, type assertions, nullish coalescing, optional chaining, conditional rendering) exercised by Ink examples, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 330 | `ink-use-effect-cleanup` | `useEffect` cleanup functions | P1 |
| 331 | `ink-use-callback-deps` | `useCallback` with dependencies | P1 |
| 332 | `ink-use-memo-deps` | `useMemo` with dependencies | P1 |
| 333 | `ink-children-only` | `Children.only` | P2 |
| 334 | `ink-children-toarray` | `Children.toArray` | P2 |
| 335 | `ink-react-lazy-fallback` | `React.lazy` with `Suspense` fallback | P2 |
| 336 | `ink-generic-function-component` | Generic function components | P1 |
| 337 | `ink-polymorphic-component` | Polymorphic components with `as` prop | P2 |
| 338 | `ink-compound-components` | Compound component pattern | P2 |
| 339 | `ink-context-reducer` | Context with `useReducer` | P1 |
| 340 | `ink-controlled-uncontrolled` | Controlled vs uncontrolled components | P1 |
| 341 | `ink-forward-ref-generic` | `forwardRef` with generic components | P2 |
| 342 | `ink-render-props-generic` | Render props with generics | P2 |
| 343 | `ink-hoc-generic` | Higher-order components with generics | P2 |
| 344 | `ink-custom-hook-composition` | Composing multiple custom hooks | P1 |
| 345 | `ink-optional-call` | Optional call expression (`fn?.()`) | P1 |
| 346 | `ink-non-null-chain` | Non-null assertion after optional chain | P1 |
| 347 | `ink-type-assertion-jsx` | Type assertions in JSX expressions | P1 |
| 348 | `ink-nullish-jsx-attr` | Nullish coalescing in JSX attributes | P1 |
| 349 | `ink-optional-chain-jsx` | Optional chaining in JSX attributes | P1 |
| 350 | `ink-jsx-conditional-render` | Ternary / logical AND returning JSX | P1 |

**Tasks:** 330–350 | **Status:** 🔄 Pending (21 tasks)

---

### Phase 28: Advanced Type System Patterns
**Goal:** Advanced TypeScript type system patterns exercised by Ink examples: mapped types with `as` clause, template literal types with unions, user-defined type guards (`is`), `satisfies` with unions, `as const` in function returns, utility types `Extract`/`Exclude`/`NonNullable`, key remapping, recursive readonly mapped types, nested conditional types, and inline `import('...').Type` syntax, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 351 | `ink-mapped-types-as` | Mapped types with `as` clause | P2 |
| 352 | `ink-template-literal-union` | Template literal types with unions | P2 |
| 353 | `ink-type-guard-is` | User-defined type guards (`is`) | P1 |
| 354 | `ink-satisfies-union` | `satisfies` with union types | P1 |
| 355 | `ink-as-const-function` | `as const` in function returns | P1 |
| 356 | `ink-utility-extract-exclude` | `Extract`, `Exclude`, `NonNullable` | P2 |
| 357 | `ink-key-remapping` | Key remapping with mapped types | P2 |
| 358 | `ink-readonly-mapped` | Recursive readonly mapped types | P2 |
| 359 | `ink-nested-conditional` | Nested conditional types | P2 |
| 360 | `ink-inline-import-type` | Inline `import('...').Type` syntax | P1 |

**Tasks:** 351–360 | **Status:** 🔄 Pending (10 tasks)

---

### Phase 29: Array/Object/String/Number/Date/Console/Error/Symbol/Process/Stream/WebAssembly API Completion
**Goal:** Final Ink examples for array methods (flat, flatMap), object methods (fromEntries), string methods (match, search, replace), RegExp well-known symbols, number formatting (toExponential, toPrecision, toFixed), date locale formatting (toLocaleDateString, toLocaleTimeString, toDateString, toTimeString), advanced console methods (assert, count, group, trace, timeLog), error stack APIs (captureStackTrace, stackTraceLimit), well-known symbols (toStringTag, toPrimitive, hasInstance, species), import.meta.env (Vite pattern), process.hrtime, fs/promises, stream/web (ReadableStream, WritableStream, TransformStream), timers/promises, and WebAssembly, validated across all 3 environments via `scripts/parity.sh` with **100% output match**.

| Task | Example | Feature | Priority |
|------|---------|---------|----------|
| 361 | `ink-array-flat-flatmap` | `Array.prototype.flat` and `flatMap` | P1 |
| 362 | `ink-object-fromentries` | `Object.fromEntries` | P1 |
| 363 | `ink-string-match-search` | `String.prototype.match`, `search`, `replace` | P1 |
| 364 | `ink-regexp-symbol` | RegExp well-known symbols | P2 |
| 365 | `ink-number-format` | `Number.prototype.toExponential`, `toPrecision`, `toFixed` | P2 |
| 366 | `ink-date-locale` | Date locale formatting methods | P2 |
| 367 | `ink-console-advanced` | `console.assert`, `count`, `group`, `trace`, `timeLog` | P2 |
| 368 | `ink-error-stack` | `Error.captureStackTrace`, `Error.stackTraceLimit` | P2 |
| 369 | `ink-symbol-wellknown` | Well-known symbols | P1 |
| 370 | `ink-import-meta-env` | `import.meta.env` (Vite pattern) | P1 |
| 371 | `ink-process-hrtime` | `process.hrtime` / `process.hrtime.bigint` | P2 |
| 372 | `ink-fs-promises` | `fs/promises` module | P2 |
| 373 | `ink-stream-web` | `stream/web` (ReadableStream, WritableStream, TransformStream) | P2 |
| 374 | `ink-timers-promises` | `timers/promises` module | P2 |
| 375 | `ink-webassembly` | `WebAssembly` API | P3 |

**Tasks:** 361–375 | **Status:** 🔄 Pending (15 tasks)

---

### Phase 30: Final Comprehensive Coverage Audit
**Goal:** End-to-end audit of all 376 tasks, verifying file-to-JSON consistency, phase coverage, 100% parity language, orphaned example tracking, and final stats publication.

| Task | Description | Priority |
|------|-------------|----------|
| 376 | Final comprehensive coverage audit | P0 |

**Tasks:** 376 | **Status:** 🔄 Pending (1 task)

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

### Features Without Examples (250 gaps)

173 examples exist. Phase 6 examples (Tasks 042–067) cover core TS/TSX/React/Ink features. Phase 10 examples (Tasks 079–099) cover extended features. Phase 11 examples (Tasks 100–120) cover type system deep features and modern runtime APIs. Phase 12 examples (Tasks 121–131) cover remaining runtime APIs and type system features. Phase 13 examples (Tasks 132–140) cover real-world validation gaps. Phase 14–16 examples (Tasks 141–178) cover final runtime API, module pattern, operator, syntax, and advanced React pattern completion. Phase 17 examples (Tasks 179–190) cover ES2024 and TypeScript 5.0+ features. Phase 18 examples (Tasks 191–205) cover expression-level features, React type patterns, advanced runtime APIs, JSX edge cases, and legacy features. Phase 19 examples (Tasks 206–220) cover tagged templates, compiler options, deep Reflect/Object coverage, Web APIs, and ES2025 features. Phase 20 examples (Tasks 221–235) cover advanced language features, Intl/Atomics APIs, compile infrastructure, JSX member/namespace expressions, and React patterns. Phase 21 examples (Tasks 236–250) cover niche language features, additional TypeScript configuration options, and Web Streams/Crypto APIs. Phase 22 examples (Tasks 251–270) cover Web APIs (WebSocket, FormData, Blob, EventTarget, FinalizationRegistry, SharedArrayBuffer), TypeScript configuration edge cases, and advanced TS/JSX language patterns. Phase 23 examples (Tasks 271–285) cover emerging runtime APIs, React patterns, module resolution, and compile infrastructure. Phase 24 examples (Tasks 286–299) cover the orphaned existing Ink example audit, advanced type system features, and language edge cases. Phase 25 examples (Tasks 300–314) cover advanced destructuring patterns and Node.js/Web runtime APIs. Phase 26 examples (Tasks 315–329) cover Node.js standard library, browser globals, binary data APIs, Promise chaining, and advanced Math. Phase 27 examples (Tasks 330–350) cover React hook patterns, JSX expression patterns, and component composition. Phase 28 examples (Tasks 351–360) cover advanced TypeScript type system patterns. Phase 29 examples (Tasks 361–375) cover array/object/string/number/date/console/error/symbol/process/stream/WebAssembly API completion. Phase 30 (Task 376) is the final comprehensive coverage audit. See `tasks/index.json` → `coverage_gaps.features_without_examples` for the full list.

Remaining gaps (Phase 10–12 targets):
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
- Modern array methods: `flat`, `flatMap`, `at`, `toSorted`, `toReversed`, `includes`, `findLast`, `toSpliced`, `with`
- Modern object methods: `fromEntries`, `hasOwn`, `getOwnPropertyDescriptors`, `create`, `defineProperty`, `freeze`, `seal`, `assign`
- Modern string methods: `padStart`, `padEnd`, `replaceAll`, `trimStart`, `trimEnd`, `at`
- Promise advanced: `allSettled`, `any`, `race`, `withResolvers`
- Globals: `Date`, `Math`, `Intl`
- Runtime APIs: `JSON.stringify`/`parse`, `queueMicrotask`, `Number.isFinite`/`isNaN`/`parseInt`/`parseFloat`
- Function methods: `bind`, `call`, `apply`
- Error: `Error.cause`
- Class: `static {}` blocks, private methods `#method()`, `#field in obj`
- React: `createRef`, `useDebugValue`
- Type system: function overloads, parameter properties, namespace re-export, inline type imports
- Runtime APIs: console methods, URI encoding, array reduce, string search, error subclasses
- **Real-world gaps (from `../tui1` audit):**
  - `process` global (`process.exit`, `process.env`, `process.stdin`, `process.stdout`)
  - `setInterval` / `clearInterval`
  - `Date` object (`new Date`, `Date.now`, `toLocaleTimeString`)
  - `Array.prototype.splice`
  - React Fragment shorthand `<>...</>`
  - Dynamic import of node built-ins (`import("node:readline")`)
  - `/** @jsxImportSource react */` pragma
  - Module-level `render()` call

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

### Pending 🔄 Phase 8–12
- [ ] Decorators parse into HIR without producing Invalid (Task 074).
- [ ] Compile-path parity tests for all examples (Task 076).
- [ ] Compile-path negative tests (Task 077).
- [ ] Coverage matrix published in `docs/SUPPORTED_SUBSET.md` (Task 078).
- [ ] 19 additional Ink examples for extended TS/TSX coverage (Tasks 079–099).
- [ ] 21 additional Ink examples for type system + runtime API deep coverage (Tasks 100–120).
- [ ] 11 additional Ink examples for runtime API + type system completion (Tasks 121–131).
- [ ] `../tui1` example compiles and renders identically in all 3 environments (Tasks 132–140).
- [ ] 8 additional Ink examples for final runtime API + module pattern completion (Tasks 141–148).
- [ ] 15 additional Ink examples for advanced React patterns + deep runtime coverage (Tasks 149–163).
- [ ] 15 additional Ink examples for operator + syntax + runtime API completion (Tasks 164–178).
- [ ] 12 additional Ink examples for ES2024 + TypeScript 5.0+ feature completion (Tasks 179–190).
- [ ] 15 additional Ink examples for expression-level + React pattern + runtime API completion (Tasks 191–205).
- [ ] 15 additional Ink examples for tagged templates + compiler options + Reflect deep coverage + ES2025 (Tasks 206–220).
- [ ] 15 additional Ink examples for advanced language + runtime + React patterns + compile infrastructure (Tasks 221–235).
- [ ] 15 additional Ink examples for niche language features + TypeScript config + Web Streams + Crypto (Tasks 236–250).
- [ ] 20 additional Ink examples for Web APIs + TypeScript config edge cases + JSX/TS language patterns (Tasks 251–270).
- [ ] 15 additional Ink examples for emerging runtime APIs + React patterns + module resolution + compile infrastructure (Tasks 271–285).
- [ ] 14 additional tasks for orphaned example audit + advanced type system + language edge cases (Tasks 286–299).
- [ ] 15 additional Ink examples for advanced destructuring + Node.js/Web runtime APIs (Tasks 300–314).
- [ ] 15 additional Ink examples for Node.js standard library + browser globals + binary data + Promise/Math completion (Tasks 315–329).
- [ ] 21 additional Ink examples for React hook patterns + JSX expression patterns + component composition (Tasks 330–350).
- [ ] 10 additional Ink examples for advanced TypeScript type system patterns (Tasks 351–360).
- [ ] 15 additional Ink examples for array/object/string/number/date/console/error/symbol/process/stream/WebAssembly API completion (Tasks 361–375).
- [ ] Final comprehensive coverage audit (Task 376).
- [ ] `scripts/parity.sh --env all` passes all examples with 100% match.
