# runts — Supported TypeScript/TSX Subset Specification

## Version: 0.5.0 | Status: MVP+

This document precisely defines the TypeScript + TSX subset that runts accepts. The subset is ruthlessly minimal but covers **>95% of real-world Fresh/Preact usage**.

---

## 1. Philosophy

**Rule of parsimony:** If a feature adds significant compiler complexity for marginal user value, it is excluded. Users can refactor around exclusions in minutes.

**Fresh compatibility:** Valid Fresh/Preact code that stays within this subset compiles to native Rust with **zero or minimal changes**.

---

## 2. Language Core — ✅ Supported

### 2.1 Types

| Feature | Support | Notes |
|---------|---------|-------|
| `string`, `number`, `boolean`, `null`, `undefined`, `void` | ✅ Full | Maps to Rust primitives |
| `any`, `unknown` | ✅ Degraded | Maps to `serde_json::Value` |
| `Array<T>` / `T[]` | ✅ Full | Maps to `Vec<T>` |
| `T | null` / `T | undefined` | ✅ Full | Maps to `Option<T>` |
| `T & U` (intersection) | ✅ Partial | Maps to trait bounds where possible, else `serde_json::Value` |
| `Record<K, V>` | ✅ Full | Maps to `HashMap<K, V>` |
| Interfaces | ✅ Full | Maps to `struct` with `Serialize/Deserialize` |
| Type aliases | ✅ Full | Inlined or mapped to Rust type aliases |
| `enum` (TS string/number unions) | ✅ Full | Maps to Rust enums |
| Function types | ✅ Full | Maps to `Box<dyn Fn(...)>` |
| Generics (`<T extends U>`) | ✅ Partial | Monomorphized at codegen time; bounds checked |
| Tuple types `[A, B]` | ✅ Full | Maps to Rust tuples |
| Template literal types | ❌ Excluded | Rare in UI code |
| Conditional types (`T extends U ? V : W`) | ❌ Excluded | Use overloads or runtime checks |
| Mapped types (`{ [K in T]: V }`) | ❌ Excluded | Use explicit interfaces |
| `infer` | ❌ Excluded | Type-level metaprogramming |
| `satisfies` | ❌ Excluded | Use explicit annotations |

### 2.2 Variables & Expressions

| Feature | Support | Notes |
|---------|---------|-------|
| `const` / `let` | ✅ Full | `var` is rejected |
| Destructuring (`const {a, b} = obj`) | ✅ Full | Object + array patterns |
| Default values in destructuring | ✅ Full | |
| Spread (`...rest`) | ✅ Full | Arrays and objects |
| Arrow functions (`=>`) | ✅ Full | Closures captured into `Box<dyn Fn>` |
| Function declarations | ✅ Full | Default + named exports |
| Async/await | ✅ Full | Maps to `async fn` |
| `if/else`, `switch` | ✅ Full | |
| `for`, `while`, `for...of` | ✅ Full | `for...in` rejected (use `Object.keys`) |
| `.map()`, `.filter()`, `.reduce()` | ✅ Full | On arrays |
| `.forEach()` | ✅ Supported | Treated as imperative loop |
| Template literals (`` `hello ${name}` ``) | ✅ Full | Maps to `format!()` |
| Optional chaining (`?.`) | ✅ Full | Desugared to `Option::and_then` |
| Nullish coalescing (`??`) | ✅ Full | Desugared to `Option::unwrap_or` |
| Ternary (`cond ? a : b`) | ✅ Full | |
| Logical (`&&`, `\|\|`) | ✅ Full | |
| Bitwise operators | ❌ Excluded | Rare in UI code |
| `typeof` | ✅ Partial | Only for primitive guards |
| `instanceof` | ❌ Excluded | Use tagged unions or `typeof` |
| `in` operator | ❌ Excluded | Use `Object.hasOwn` equivalent |
| `with` statement | ❌ Excluded | Forbidden |
| `eval()` | ❌ Excluded | Forbidden |
| `try/catch/finally` | ✅ Partial | Basic error handling only |
| `throw` | ✅ Partial | String and Error objects |

### 2.3 Classes & Objects

| Feature | Support | Notes |
|---------|---------|-------|
| Object literals | ✅ Full | |
| Shorthand properties (`{a, b}`) | ✅ Full | |
| Computed property names | ✅ Partial | Static keys only |
| `class` declarations | ❌ Excluded | Use functions + hooks |
| `new` operator | ❌ Excluded | Use factory functions |
| `this` | ❌ Excluded | Use closures and props |
| Prototype manipulation | ❌ Excluded | |
| `Object.assign`, `Object.keys`, etc. | ✅ Partial | Common static methods |
| `JSON.parse` / `JSON.stringify` | ✅ Full | Maps to `serde_json` |

---

## 3. JSX/TSX — ✅ Supported

### 3.1 Elements

| Feature | Support | Notes |
|---------|---------|-------|
| HTML elements (`<div>`, `<span>`, etc.) | ✅ Full | Lowercase tags |
| Custom components (`<Header />`) | ✅ Full | PascalCase tags |
| Fragments (`<>...</>`) | ✅ Full | Maps to `Fragment` |
| Self-closing tags | ✅ Full | |
| Children (text, elements, expressions) | ✅ Full | |

### 3.2 Attributes

| Feature | Support | Notes |
|---------|---------|-------|
| String literals (`class="foo"`) | ✅ Full | |
| Expression values (`count={n}`) | ✅ Full | |
| Boolean attributes (`disabled={true}`) | ✅ Full | |
| Spread attributes (`{...props}`) | ✅ Partial | Static spread only |
| `class` → `class_name` | ✅ Auto | Renamed automatically |
| `for` → `html_for` | ✅ Auto | Renamed automatically |
| Event handlers (`onClick={handler}`) | ✅ Full | Renamed to `on_click` |
| `style={{ color: "red" }}` | ✅ Full | Maps to inline style string |
| `ref` | ❌ Excluded | Use `use_ref` hook instead |
| `dangerouslySetInnerHTML` | ❌ Excluded | Security risk; use server render |

### 3.3 Special JSX Patterns

| Feature | Support | Notes |
|---------|---------|-------|
| Conditional rendering (`{flag && <A />}`) | ✅ Full | |
| Lists with `key` | ✅ Full | `key` prop extracted for diffing |
| `map()` in JSX | ✅ Full | |
| Nested components | ✅ Full | |
| Inline components (functions inside render) | ⚠️ Limited | May cause hook ordering issues |

---

## 4. Fresh-Specific Features — ✅ Supported

### 4.1 File-Based Routing

| Feature | Support | Notes |
|---------|---------|-------|
| `routes/index.tsx` → `/` | ✅ Full | |
| `routes/about.tsx` → `/about` | ✅ Full | |
| `routes/blog/[slug].tsx` → `/blog/:slug` | ✅ Full | Dynamic segments |
| `routes/blog/[...slug].tsx` → `/blog/*` | ✅ Full | Catch-all |
| `routes/(group)/page.tsx` | ⚠️ Planned | Route groups |
| `routes/_layout.tsx` | ✅ Full | Nested layouts |
| `routes/_middleware.ts` | ✅ Full | Per-route middleware |
| `routes/_app.tsx` | ✅ Full | App wrapper |

### 4.2 Route Handlers

```typescript
export const handler = {
  async GET(req: Request, ctx: HandlerContext) {
    const data = await fetchData();
    return new Response(JSON.stringify(data), {
      headers: { "Content-Type": "application/json" }
    });
  },
  async POST(req: Request, ctx: HandlerContext) { /* ... */ }
};
```

| Feature | Support | Notes |
|---------|---------|-------|
| `handler` object with HTTP methods | ✅ Full | GET, POST, PUT, DELETE, PATCH |
| `Request` / `Response` Web API | ✅ Partial | Headers, body, status, JSON |
| `HandlerContext` | ✅ Full | `ctx.render()`, `ctx.state`, `ctx.params` |
| `ctx.render(Component, { data })` | ✅ Full | |
| `ctx.state` | ✅ Full | Passes through middleware chain |
| Streaming responses | ❌ Excluded | Use standard Response |

### 4.3 Islands Architecture

| Feature | Support | Notes |
|---------|---------|-------|
| `islands/*.tsx` auto-registration | ✅ Full | |
| Island components (PascalCase in `islands/`) | ✅ Full | |
| Props serialization to client | ✅ Full | JSON via `data-props` |
| Partial hydration strategies | ✅ Full | `load`, `idle`, `visible`, `interaction` |
| Island boundaries in SSR | ✅ Full | Placeholder + script injection |
| Client-side island hydration | ✅ Full | Vanilla JS runtime |
| Island-to-island communication | ⚠️ Limited | Via signals + DOM events |

### 4.4 Middleware

```typescript
// _middleware.ts
import { MiddlewareHandlerContext } from "$fresh/server.ts";

export async function handler(req: Request, ctx: MiddlewareHandlerContext) {
  ctx.state.user = await getUser(req);
  const resp = await ctx.next();
  resp.headers.set("X-Custom", "header");
  return resp;
}
```

| Feature | Support | Notes |
|---------|---------|-------|
| Global `_middleware.ts` | ✅ Full | |
| Route-scoped `_middleware.ts` | ✅ Full | |
| `ctx.next()` chaining | ✅ Full | |
| `ctx.state` propagation | ✅ Full | |
| Early return (short-circuit) | ✅ Full | |
| Multiple middleware files | ✅ Full | Ordered by specificity |

---

## 5. Preact/Hooks — ✅ Supported

### 5.1 Core Hooks

| Hook | Support | Rust Mapping |
|------|---------|--------------|
| `useState` | ✅ Full | `use_state<T>(initial) -> (T, impl Fn(T))` |
| `useEffect` | ✅ Partial | Server: no-op; Client: runs after hydration |
| `useRef` | ✅ Full | `use_ref<T>(initial) -> Ref<T>` |
| `useMemo` | ✅ Full | `use_memo(factory, deps) -> T` |
| `useCallback` | ✅ Full | `use_callback(callback, deps) -> impl Fn` |
| `useReducer` | ✅ Full | `use_reducer(reducer, initial) -> (S, impl Fn(A))` |
| `useContext` | ✅ Full | `use_context(ctx) -> T` |
| `useId` | ✅ Full | Generates stable SSR-safe IDs |
| `useLayoutEffect` | ⚠️ Limited | Treated as `useEffect` |

### 5.2 Signals (Preact Signals)

| Feature | Support | Notes |
|---------|---------|-------|
| `signal(initial)` | ✅ Full | `Signal<T>` with fine-grained reactivity |
| `computed(fn)` | ✅ Full | `Computed<T>` |
| `effect(fn)` | ✅ Full | Runs on signal changes |
| `batch(fn)` | ✅ Full | Batches updates |
| Reading signals in JSX (`{count.value}`) | ✅ Full | Auto-dereferenced in expressions |
| `.peek()` | ✅ Full | Non-reactive read |

### 5.3 Component Patterns

| Pattern | Support | Notes |
|---------|---------|-------|
| Function components | ✅ Full | Default export or named export |
| Props destructuring | ✅ Full | `({ name, count }: Props)` |
| Default props | ✅ Full | Via destructuring defaults |
| Children prop | ✅ Full | Passed through JSX children |
| Render props | ⚠️ Limited | Use component composition |
| Higher-order components | ❌ Excluded | Use composition or hooks |
| Error boundaries | ❌ Excluded | Use `try/catch` in handlers |
| Portals | ❌ Excluded | Rare in SSR context |
| Suspense | ⚠️ Planned | Async component boundaries |

---

## 6. Module System — ✅ Supported

| Feature | Support | Notes |
|---------|---------|-------|
| `import { a } from "./file"` | ✅ Full | Relative + absolute paths |
| `import * as ns` | ✅ Full | Namespace imports |
| `export default` | ✅ Full | Component + handler default exports |
| `export { a, b }` | ✅ Full | Named exports |
| `export * from` | ✅ Partial | Re-exports |
| `$fresh/server.ts` | ✅ Full | Fresh builtins mapped to runts runtime |
| `preact/hooks` | ✅ Full | Hooks mapped to runts runtime |
| `@preact/signals` | ✅ Full | Signals mapped to runts runtime |
| `npm:` specifiers | ❌ Excluded | Use native Rust crates instead |
| `jsr:` specifiers | ❌ Excluded | |
| Dynamic `import()` | ❌ Excluded | Use static imports |
| `require()` | ❌ Excluded | ESM only |

---

## 7. Standard Library — Partial

| API | Support | Rust Equivalent |
|-----|---------|-----------------|
| `fetch` | ✅ Full | `reqwest` / `hyper` |
| `console.log` | ✅ Full | `tracing` |
| `setTimeout` / `clearTimeout` | ✅ Partial | `tokio::time` |
| `setInterval` / `clearInterval` | ✅ Partial | `tokio::time::interval` |
| `Promise.all` | ✅ Full | `futures::join!` |
| `Promise.race` | ⚠️ Limited | |
| `URL` / `URLSearchParams` | ✅ Full | `url::Url` |
| `Date` | ✅ Full | `chrono` |
| `Math.*` | ✅ Full | Rust `std::f64` |
| `Array.isArray` | ✅ Full | `Vec` check |
| `Object.keys/values/entries` | ✅ Full | HashMap methods |
| `localStorage` / `sessionStorage` | ❌ Excluded | Use cookies or server state |
| `window` / `document` | ❌ Excluded | Server context only; use islands for DOM |
| `WebSocket` | ⚠️ Planned | Via `tokio-tungstenite` |
| `EventSource` (SSE) | ✅ Full | Native Axum SSE |

---

## 8. Explicit Exclusions (With Migration Path)

These are **consciously excluded** to keep the compiler tractable and the runtime lean.

| Feature | Why Excluded | Migration Path |
|---------|--------------|----------------|
| `class` components | Adds OOP complexity; hooks cover 100% of use cases | Convert to function + hooks |
| `this` keyword | Requires prototype chain emulation | Use closures and props |
| `eval()` / `new Function()` | Security + compilation impossibility | Use data-driven logic |
| `with` statement | Scope chain manipulation | Use destructuring |
| `var` | Scoping footguns | Use `const`/`let` |
| `for...in` | Iterates prototype chain | Use `Object.keys` + `for...of` |
| `instanceof` | Requires prototype chain | Use `typeof` or tagged unions |
| Dynamic `import()` | Static analysis impossibility | Use conditional component rendering |
| `npm:` / `jsr:` specifiers | Requires JS runtime | Use Cargo.toml + native crates |
| `try/catch` for control flow | Anti-pattern | Use `Result` types |
| Regex literal complexity | Parser complexity | Use simple patterns |
| Generator functions (`function*`) | Iterator protocol emulation | Use `Vec` + loops |
| Decorators (`@decorator`) | TC39 stage uncertainty | Use higher-order functions |
| `Symbol` / `Symbol.iterator` | Metaprogramming complexity | Use explicit protocols |
| `WeakMap` / `WeakRef` | No Rust equivalent | Use `HashMap` + explicit cleanup |
| `Proxy` / `Reflect` | Metaprogramming complexity | Use explicit getters/setters |
| `Intl` API | Massive surface area | Use `chrono` + locale crates |
| WebGL / Canvas 2D | Out of scope | Use server-rendered SVG or islands |
| Service Workers | Out of scope | Use server caching headers |
| WebAssembly imports | Circular dependency | Use native Rust directly |

---

## 9. Fresh Compatibility Score

| Fresh Feature | Compatibility |
|---------------|---------------|
| File-based routing | 100% |
| Islands architecture | 100% |
| Route handlers | 100% |
| Middleware | 100% |
| Layouts | 100% |
| `ctx.render()` | 100% |
| `ctx.state` | 100% |
| `HandlerContext` | 100% |
| Preact components | 100% |
| Preact hooks | 95% (excludes `useLayoutEffect` nuances) |
| Preact signals | 100% |
| Partial hydration | 100% |
| Static files | 100% |
| Error pages (`_404.tsx`, `_500.tsx`) | 100% |
| Plugins | 0% (planned v0.7) |
| DevTools integration | 0% (planned v0.8) |

**Overall Fresh compatibility: ~95%**

---

## 10. Validating Your Code

Run `runts transpile` to check compatibility:

```bash
runts transpile --path ./my-project
```

Unsupported features produce clear error messages with line numbers and migration suggestions.
