# runts — Supported TypeScript/TSX Subset Specification

> **Version:** 0.5.0  
> **Status:** Authoritative  
> **Goal:** Cover 95%+ of real-world Fresh/Preact usage with a minimal, well-defined, efficiently-compilable subset.

---

## 1. Design Principles

1. **Completeness over Convenience** — Support the patterns Fresh/Preact users actually write. Exclude edge-case language features that complicate codegen.
2. **Zero Runtime Reflection** — Every construct must have a direct, static Rust codegen path. No `eval`, no `new Function`, no dynamic property access that cannot be devirtualized.
3. **Type-Erasure at Runtime** — TypeScript types are fully erased. They inform codegen (struct shapes, enum variants) but disappear at runtime.
4. **Functional-First** — Only function components. No classes, no prototypes, no `this`.
5. **Predictable Memory** — No closures escaping into dynamically-typed containers. Closure captures are statically known.

---

## 2. Supported Language Features

### 2.1 Variables & Binding

| Feature | Syntax | Rust Equivalent | Notes |
|---------|--------|-----------------|-------|
| `const` | `const x = 5` | `let x = 5;` | Immutable binding |
| `let` | `let x = 5` | `let mut x = 5;` | Mutable binding |
| `var` | `var x = 5` | `let mut x = 5;` | Hoisting is flattened; treated as `let` |
| Destructuring (object) | `const {a, b} = obj` | `let a = obj.a; let b = obj.b;` | Nested supported |
| Destructuring (array) | `const [a, b] = arr` | `let a = arr[0]; let b = arr[1];` | Rest patterns supported |
| Rest in destructuring | `const {a, ...rest} = obj` | Named struct + HashMap capture | |
| Default values | `const {a = 1} = obj` | `unwrap_or(1)` | |

### 2.2 Functions

| Feature | Syntax | Status | Notes |
|---------|--------|--------|-------|
| Function declaration | `function foo() {}` | ✅ Full | |
| Arrow functions | `const f = () => {}` | ✅ Full | Including async arrows |
| Async/await | `async function foo() {}` | ✅ Full | Translates to `async fn` |
| Default parameters | `function f(x = 1)` | ✅ Full | |
| Rest parameters | `function f(...args)` | ✅ Full | Translates to `Vec<T>` |
| Generator functions | `function* gen() {}` | ❌ Excluded | No `yield` in subset |

### 2.3 Expressions & Operators

| Feature | Status | Rust Equivalent | Notes |
|---------|--------|-----------------|-------|
| Arithmetic (`+`, `-`, `*`, `/`, `%`) | ✅ Full | Same | String concatenation with `+` supported |
| Comparison (`==`, `===`, `!=`, `!==`, `<`, `<=`, `>`, `>=`) | ✅ Full | `==` / `!=` coerce; `===` is strict eq | |
| Logical (`&&`, `\|\|`, `??`) | ✅ Full | `&&` / `\|\|` short-circuit; `??` → `unwrap_or` | |
| Bitwise | ⚠️ Partial | Supported in codegen | Rare in Fresh apps |
| Unary (`!`, `-`, `+`, `typeof`, `void`) | ✅ Full | `typeof` → runtime type tag | |
| Optional chaining | `obj?.prop?.method()` | ✅ Full | Chains desugar to nested `match` |
| Nullish coalescing | `a ?? b` | ✅ Full | `Option::unwrap_or` |
| Ternary | `cond ? a : b` | ✅ Full | `if/else` |
| Template literals | `` `hello ${name}` `` | ✅ Full | `format!` |
| Tagged templates | `` css`...` `` | ❌ Excluded | Rare; use objects instead |

### 2.4 Control Flow

| Feature | Status | Notes |
|---------|--------|-------|
| `if` / `else` | ✅ Full | |
| `switch` | ✅ Full | Translates to `match` |
| `for` (C-style) | ✅ Full | |
| `for...of` | ✅ Full | `for item in &collection` |
| `while` | ✅ Full | |
| `break` / `continue` | ✅ Full | |
| `try` / `catch` / `finally` | ⚠️ Partial | Allowed in handlers/effects, NOT in render path |
| `return` | ✅ Full | Early returns supported |

### 2.5 Data Structures

| Feature | Status | Rust Equivalent | Notes |
|---------|--------|-----------------|-------|
| Object literals | ✅ Full | Named structs (inferred) or `HashMap` | Shorthand `{a}`, spread `{...obj}` |
| Array literals | ✅ Full | `Vec<T>` | Heterogeneous arrays → `Vec<serde_json::Value>` |
| Spread in array | `[...a, b]` | ✅ Full | `extend` + `push` |
| Spread in object | `{...a, b: 1}` | ✅ Full | `clone` + `insert` |
| Tuple-like arrays | ⚠️ Partial | Infer as tuple if fixed size | |

### 2.6 Module System

| Feature | Status | Notes |
|---------|--------|-------|
| ES `import` / `export` | ✅ Full | Static only |
| Named imports | `import { a } from "x"` | ✅ Full | |
| Default imports | `import X from "x"` | ✅ Full | |
| Namespace imports | `import * as X from "x"` | ✅ Full | Module treated as struct |
| Re-exports | `export { a } from "x"` | ✅ Full | |
| `export default` | ✅ Full | Required for page components |
| `export const handler` | ✅ Full | Fresh route handler convention |
| Dynamic `import()` | ❌ Excluded | Use static imports |
| `require()` | ❌ Excluded | CommonJS not supported |
| `module.exports` | ❌ Excluded | ES modules only |

---

## 3. Supported JSX/TSX

### 3.1 Elements & Components

| Feature | Example | Status | Notes |
|---------|---------|--------|-------|
| HTML elements | `<div>...</div>` | ✅ Full | All HTML5 + SVG tags |
| Components | `<Counter />` | ✅ Full | PascalCase = component |
| Fragments | `<>...</>` | ✅ Full | `<Fragment>` alias too |
| Self-closing | `<img />` | ✅ Full | |
| Children | `<A>{child}</A>` | ✅ Full | Text, expressions, elements |
| Spread props | `<div {...props} />` | ✅ Full | |
| Boolean attrs | `<input disabled />` | ✅ Full | |
| Data attrs | `data-id="x"` | ✅ Full | |
| ARIA attrs | `aria-label="x"` | ✅ Full | |
| Inline styles | `style={{color: "red"}}` | ✅ Full | Object → CSS string |
| `class` / `className` | ✅ Full | Unified to `class` in output |
| `key` prop | `<div key={i} />` | ✅ Full | List reconciliation hint |
| `ref` | `<div ref={myRef} />` | ⚠️ Deferred | v0.6 |
| Dynamic tags | `<{tagName} />` | ⚠️ Deferred | v0.6; requires runtime dispatch |

### 3.2 JSX Expressions

| Feature | Example | Status | Notes |
|---------|---------|--------|-------|
| Text interpolation | `{name}` | ✅ Full | Escaped |
| Conditional rendering | `{flag && <A />}` | ✅ Full | `flag` must be boolean |
| Ternary in JSX | `{flag ? <A /> : <B />}` | ✅ Full | |
| Null rendering | `{null}` | ✅ Full | Renders nothing |
| Array mapping | `{items.map(x => <X />)}` | ✅ Full | Requires `key` prop |
| Arrow in JSX | `{items.filter(...).map(...)}` | ✅ Full | Inline closures |
| IIFE in JSX | `{(() => { ... })()}` | ⚠️ Partial | Allowed if pure |

### 3.3 Event Handlers (Islands Only)

| Feature | Example | Status | Notes |
|---------|---------|--------|-------|
| Click handlers | `onClick={handler}` | ✅ Full | Islands only |
| Input handlers | `onInput={e => ...}` | ✅ Full | |
| Submit handlers | `onSubmit={handleSubmit}` | ✅ Full | |
| Keyboard handlers | `onKeyDown={...}` | ✅ Full | |
| Custom events | `onCustom={...}` | ⚠️ Partial | Standard DOM events only |
| Event object access | `e.target.value` | ✅ Full | Synthetic event shim |

---

## 4. Supported TypeScript Types

### 4.1 Type Annotations (Fully Erased at Runtime)

| TypeScript | Rust Equivalent | Notes |
|------------|-----------------|-------|
| `string` | `String` | |
| `number` | `f64` | Integer inference for literals |
| `boolean` | `bool` | |
| `null` | `Option<T>::None` | |
| `undefined` | `()` | Unit type |
| `T[]` | `Vec<T>` | |
| `[T, U]` | `(T, U)` | Fixed-size tuple |
| `{a: T}` | Named struct / `HashMap` | Struct if shape is known |
| `Record<K,V>` | `HashMap<K, V>` | |
| `Map<K,V>` | `HashMap<K, V>` | |
| `Set<T>` | `HashSet<T>` | |
| `Date` | `chrono::DateTime<Utc>` | |
| `Promise<T>` | `impl Future<Output = T>` | |
| `void` | `()` | |
| `any` | `serde_json::Value` | Escape hatch; discouraged |
| `unknown` | `serde_json::Value` | Same as `any` in codegen |
| `never` | `!` | Diverging type |

### 4.2 Type Declarations

| Feature | Status | Notes |
|---------|--------|-------|
| `interface` | ✅ Full | Extends only (no `implements`) |
| `type` alias | ✅ Full | Unions, intersections, literals |
| `enum` | ✅ Full | String enums preferred; numeric supported |
| `const enum` | ✅ Full | Inlined |
| Generic functions | ⚠️ Partial | Monomorphized at codegen time |
| Generic interfaces | ⚠️ Partial | Limited to one level |
| Conditional types | ❌ Excluded | `T extends U ? X : Y` |
| Mapped types | ❌ Excluded | `{ [K in T]: V }` |
| Template literal types | ❌ Excluded | `` `hello ${T}` `` |
| `infer` | ❌ Excluded | |
| `declare module` | ❌ Excluded | |
| `namespace` | ❌ Excluded | Use ES modules |
| `abstract class` | ❌ Excluded | No classes |
| Decorators | ❌ Excluded | Stage-2; not needed for Fresh |

### 4.3 Fresh-Specific Types

| Type | Status | Notes |
|------|--------|-------|
| `PageProps<P>` | ✅ Full | Params + data + state |
| `HandlerContext<S>` | ✅ Full | State typing |
| `FreshContext<S>` | ✅ Full | Middleware context |
| `RouteContext` | ✅ Full | |
| `JSX.Element` | ✅ Full | Maps to `VNode` |
| `ComponentChildren` | ✅ Full | Maps to `Vec<VNode>` |

---

## 5. Supported Preact Hooks

All hooks are implemented with Rust semantics (not a JS engine).

| Hook | Signature (TS) | Rust Equivalent | Status |
|------|---------------|-----------------|--------|
| `useState` | `useState<T>(init)` | `use_state::<T>(init)` | ✅ Full |
| `useEffect` | `useEffect(fn, deps?)` | `use_effect(fn, deps)` | ✅ Full |
| `useRef` | `useRef<T>(init)` | `use_ref::<T>(init)` | ✅ Full |
| `useMemo` | `useMemo<T>(fn, deps)` | `use_memo::<T>(fn, deps)` | ✅ Full |
| `useCallback` | `useCallback<T>(fn, deps)` | `use_callback::<T>(fn, deps)` | ✅ Full |
| `useReducer` | `useReducer<R, A>(reducer, init)` | `use_reducer::<R, A>(reducer, init)` | ✅ Full |
| `useContext` | `useContext<T>(ctx)` | `use_context::<T>(ctx)` | ✅ Full |
| `useId` | `useId()` | `use_id()` | ✅ Full |
| `useSignal` | `useSignal<T>(init)` | `Signal::new(init)` | ✅ Full |
| `useComputed` | `useComputed<T>(fn)` | `Computed::new(fn)` | ✅ Full |
| `useSignalEffect` | `useSignalEffect(fn)` | `effect(fn)` | ✅ Full |
| `useLayoutEffect` | `useLayoutEffect(fn, deps?)` | ⚠️ Deferred | v0.6; SSR noop |
| `useImperativeHandle` | `useImperativeHandle(ref, fn)` | ⚠️ Deferred | v0.6 |
| `useSyncExternalStore` | `useSyncExternalStore(sub, get, ss)` | ⚠️ Partial | SSR snapshot only |

### 5.1 Hook Rules (Enforced by Analyzer)

1. **Top-level only** — Hooks must be called at the top level of a component or custom hook, never inside loops/conditions.
2. **Consistent order** — Hook call order must be identical across renders. Analyzer verifies this statically.
3. **No hooks in non-component functions** — Calling `useState` in a regular utility function is a compile-time error.

---

## 6. Supported Fresh Framework Features

### 6.1 File-Based Routing

| Pattern | Example Route | File | Status |
|---------|--------------|------|--------|
| Static | `GET /about` | `routes/about.tsx` | ✅ |
| Index | `GET /blog` | `routes/blog/index.tsx` | ✅ |
| Dynamic param | `GET /blog/:slug` | `routes/blog/[slug].tsx` | ✅ |
| Multiple params | `GET /:year/:month` | `routes/[year]/[month].tsx` | ✅ |
| Catch-all | `GET /docs/*path` | `routes/docs/[...path].tsx` | ✅ |
| Layout | Wraps subtree | `routes/_layout.tsx` | ✅ |
| Section layout | Wraps `/blog/*` | `routes/blog/_layout.tsx` | ✅ |
| App wrapper | Wraps all | `routes/_app.tsx` | ✅ |
| Middleware | Runs before routes | `routes/_middleware.ts` | ✅ |
| Error pages | 404 / 500 | `routes/_404.tsx`, `routes/_500.tsx` | ✅ |

### 6.2 Route Handlers

```typescript
export const handler = {
  GET: async (req: Request, ctx: HandlerContext) => { ... },
  POST: async (req: Request, ctx: HandlerContext) => { ... },
  PUT: async (req: Request, ctx: HandlerContext) => { ... },
  DELETE: async (req: Request, ctx: HandlerContext) => { ... },
};
```

- ✅ Handler objects with HTTP method keys
- ✅ `ctx.render(data)` for page rendering
- ✅ `ctx.params` for route parameters
- ✅ `ctx.state` for middleware-passed state
- ✅ `new Response(body, { status, headers })` for raw responses
- ✅ Async handlers

### 6.3 Islands Architecture

| Feature | Status | Notes |
|---------|--------|-------|
| `islands/` directory | ✅ Full | Auto-detected |
| Default export = island | ✅ Full | |
| Props serialization | ✅ Full | JSON via `serde` |
| SSR placeholder | ✅ Full | Server renders HTML, client hydrates |
| Hydration: `eager` | ✅ Full | Immediate on load |
| Hydration: `visible` | ✅ Full | `IntersectionObserver` (default) |
| Hydration: `idle` | ✅ Full | `requestIdleCallback` |
| Hydration: `manual` | ✅ Full | Explicit trigger |
| Hydration: `static` | ✅ Full | Never hydrate |
| Island inside island | ⚠️ Partial | Allowed but not recommended |
| Island props from route data | ✅ Full | Pass through component props |

### 6.4 Middleware

```typescript
// routes/_middleware.ts
export async function handler(req: Request, ctx: FreshContext<State>) {
  ctx.state.user = await getUser(req);
  const resp = await ctx.next();
  resp.headers.set("X-Custom", "value");
  return resp;
}
```

- ✅ Global middleware (`routes/_middleware.ts`)
- ✅ Section middleware (`routes/blog/_middleware.ts`)
- ✅ `ctx.next()` chaining
- ✅ `ctx.state` typing and propagation
- ✅ Response mutation after `ctx.next()`
- ✅ Early returns (`return new Response(...)`)

---

## 7. Explicitly Excluded Features

These features are **not supported** and will produce a compile-time error with a helpful message.

### 7.1 Language Exclusions

| Feature | Reason | Workaround |
|---------|--------|------------|
| `class` / `new` | No OOP runtime; complicates memory layout | Use functions + closures |
| `this` | No class/instance context | Pass state explicitly |
| `prototype` | No prototype chain | Use plain objects |
| `with` statement | Not representable in Rust | Destructure instead |
| `eval()` | Security + no reflection | Use data-driven logic |
| `new Function()` | No runtime codegen | Static functions |
| `Symbol` | No symbol type in Rust | Use string keys |
| `Proxy` | No interception | Explicit getter/setter functions |
| `Reflect` | No reflection | Direct calls |
| `WeakMap` / `WeakSet` | No weak references in safe Rust | Use `HashMap` + manual cleanup |
| `Iterator` protocol | No JS iterators | Use `Vec` + `for` loops |
| `Generator` / `yield` | Complex control-flow transform | Use `async` + channels |
| `try/catch` in render | Exceptions in render path are undefined behavior | Handle errors before returning JSX |
| `delete` operator | No partial struct deletion | Reconstruct object without key |
| `in` operator | No prototype chain | `obj.contains_key(key)` pattern |
| `instanceof` | No class hierarchy | Use discriminated unions |

### 7.2 Module Exclusions

| Feature | Reason | Workaround |
|---------|--------|------------|
| Dynamic `import()` | Requires bundler/runtime | Static imports only |
| `require()` | CommonJS | ES modules |
| `import.meta` | No module metadata needed | Config via `runts.config.json` |
| Circular imports | Complicates codegen | Refactor to DAG |

### 7.3 Type System Exclusions

| Feature | Reason | Workaround |
|---------|--------|------------|
| Conditional types | Infinite expansion risk | Use explicit unions |
| Mapped types | Complex codegen | Generate structs manually |
| Template literal types | No string-level types in Rust | Use `&str` |
| `infer` | Type extraction | Explicit type params |
| `declare global` | No ambient declarations | Import types explicitly |
| Triple-slash refs | Legacy | ES modules |

### 7.4 JSX Exclusions

| Feature | Reason | Workaround |
|---------|--------|------------|
| Dynamic tag names | `<{variable} />` | Conditional rendering with known tags |
| `JSX.ElementType` | Type-only | Not needed at runtime |
| Spread with computed key | `{[key]: value}` | Explicit props object |
| `dangerouslySetInnerHTML` | Security | Use server-rendered static HTML |

---

## 8. Type-to-Rust Mapping Reference

```
TypeScript                          → Rust
─────────────────────────────────────────────────────────────
string                              → String
number                              → f64
boolean                             → bool
null                                → Option<T>::None
undefined                           → ()
T | null | undefined                → Option<T>
T[]                                 → Vec<T>
[T, U]                              → (T, U)
{ a: string, b: number }            → struct { a: String, b: f64 }
Record<string, T>                   → HashMap<String, T>
Map<K, V>                           → HashMap<K, V>
Set<T>                              → HashSet<T>
Date                                → chrono::DateTime<Utc>
Promise<T>                          → impl Future<Output = T>
async function                      → async fn
() => void                          → Fn() or FnMut()
JSX.Element                         → VNode
React.ReactNode                     → VNode | String | ()
PageProps<Data>                     → PageProps<Data>
HandlerContext<State>               → HandlerContext<State>
FreshContext<State>                 → FreshContext<State>
Signal<T>                           → Signal<T>
Computed<T>                         → Computed<T>
Ref<T>                              → Ref<T>
```

---

## 9. Validation & Error Codes

The analyzer emits specific errors for unsupported features:

| Code | Meaning | Resolution |
|------|---------|------------|
| E001 | Parse error | Check TS/TSX syntax |
| E002 | Type error | Fix type annotations |
| E003 | **Unsupported feature** | See exclusion list above |
| E004 | Island in route file | Move component to `islands/` |
| E005 | Missing handler | Export `handler` for data routes |
| E006 | Invalid route pattern | Fix file name in `routes/` |
| E007 | Import error | Check import paths |
| E008 | Build error | Check generated Rust |
| E009 | Hook rule violation | Call hooks at top level only |
| E010 | Class component detected | Convert to function component |
| E011 | Dynamic import found | Use static `import` |
| E012 | `eval` or `new Function` | Remove; use static logic |

---

## 10. Migration Guide from Full TypeScript

If you have existing Fresh/Preact code that uses excluded features:

1. **Classes → Functions**: Convert `class MyComp extends Component` to `function MyComp(props)`.
2. **`this` → Props/Closures**: Pass callbacks and refs through props.
3. **Dynamic imports → Static imports**: Move imports to top level; use conditional rendering.
4. **`eval` → Data tables**: Replace `eval("obj." + key)` with `obj[key]` (if `key` is in supported subset).
5. **Generators → Async/Await**: Replace `yield` with `async` + channels or signals.
6. **`instanceof` → Discriminated unions**: Add a `kind` or `type` field to interfaces.

---

*Last updated: 2026-05-27*
