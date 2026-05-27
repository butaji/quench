# runts Supported TypeScript/TSX Subset Specification

> Version: 0.5.0  
> Coverage target: 95%+ of real-world Fresh/Preact patterns  
> Philosophy: Minimal but sufficient. Every included feature has a direct, efficient Rust equivalent.

---

## 1. Design Principles

1. **Zero-cost abstraction** — Every TS construct maps to a Rust construct with identical semantics and no runtime overhead.
2. **No JS runtime dependency** — Nothing requires a JS engine at build time or runtime.
3. **Fresh/Preact API parity** — Code written for Fresh should work in runts with only import-path changes.
4. **Explicit exclusions** — We reject unsupported constructs at parse time with actionable error messages.

---

## 2. Supported Language Features

### 2.1 Types & Type Annotations

| Feature | Status | Rust Mapping | Notes |
|---------|--------|--------------|-------|
| `interface` | ✅ Full | `struct` + `trait` | Supports optional props, nested interfaces |
| `type` alias | ✅ Full | `type` | Limited to structural types |
| Primitive types | ✅ Full | Direct mapping | `string`→`String`, `number`→`f64`, `boolean`→`bool`, `null`/`undefined`→`Option<T>` |
| Arrays `T[]` | ✅ Full | `Vec<T>` | |
| Records `{[k: string]: T}` | ✅ Full | `HashMap<String, T>` | |
| Union types `A \| B` | ✅ Limited | `enum` (tagged) | Only discriminated unions with `kind`/`type` tag |
| Optional params `x?: T` | ✅ Full | `Option<T>` | |
| Generics `<T>` | ✅ Limited | Monomorphization | Only for built-in types (`Array<T>`, `Promise<T>`) |
| Function types | ✅ Full | `Fn(...) -> T` | Closures and function pointers |
| `void` | ✅ Full | `()` | |
| `any` | ⚠️ Escaped | `serde_json::Value` | Allowed only in handler contexts |
| `unknown` | ✅ Full | `serde_json::Value` | |
| Mapped types | ❌ Excluded | — | Use explicit interfaces |
| Conditional types | ❌ Excluded | — | |
| Template literal types | ❌ Excluded | — | |

### 2.2 Statements & Expressions

| Feature | Status | Rust Mapping |
|---------|--------|--------------|
| `const` / `let` | ✅ Full | `let` (immutable by default, `mut` inferred from usage) |
| `var` | ❌ Excluded | Error: use `let` or `const` |
| Destructuring `{a, b}` / `[a, b]` | ✅ Full | Pattern matching |
| `if` / `else` | ✅ Full | `if` / `else` |
| Ternary `cond ? a : b` | ✅ Full | `if` / `else` |
| `for` loops | ✅ Full | `for` (range or iterator) |
| `while` loops | ✅ Full | `while` |
| `for...of` | ✅ Full | `for x in xs` |
| `for...in` | ❌ Excluded | Error: use `Object.keys()` + `for...of` |
| `switch` | ✅ Limited | `match` | Only literal cases, no fall-through |
| `try` / `catch` / `throw` | ✅ Full | `Result<T, E>` / `panic!` | Panics in islands become console errors |
| `return` | ✅ Full | `return` |
| Arrow functions `() => {}` | ✅ Full | Closures `\|...\|` |
| Function declarations | ✅ Full | `fn` |
| Async functions `async fn` | ✅ Full | `async fn` |
| `await` | ✅ Full | `.await` |
| `import` / `export` | ✅ Full | `use` / `pub` |
| Default exports | ✅ Full | Module-level binding |
| Named exports | ✅ Full | `pub` bindings |
| `export * from` | ⚠️ Partial | Re-exports | Manual re-export required |
| Spread `...obj` | ✅ Full | `..obj` in struct literals |
| Rest params `...args` | ✅ Full | `Vec<T>` |
| Template literals `` `x: ${x}` `` | ✅ Full | `format!` |
| Tagged templates | ❌ Excluded | — |
| `typeof` operator | ⚠️ Partial | Type inference | Runtime `typeof` not supported |
| `instanceof` | ❌ Excluded | — | Use discriminated unions |
| `in` operator | ❌ Excluded | — | Use `Object.hasOwn()` |

### 2.3 JSX/TSX

| Feature | Status | Rust Mapping | Notes |
|---------|--------|--------------|-------|
| Element creation `<div />` | ✅ Full | `html!` macro | |
| Attributes `class="x"` | ✅ Full | Named arguments | `className`/`class` → `class_name` |
| Spread attributes `{...props}` | ✅ Full | `..props` | |
| Boolean attributes | ✅ Full | Boolean args | `disabled` → `disabled = true` |
| Event handlers `onClick={fn}` | ✅ Full | Closure fields | `on_click`, `on_input`, `on_change`, `on_submit` |
| Event handler types | ✅ Limited | Synthetic events | `MouseEvent`, `InputEvent`, `SubmitEvent` |
| Children `{expr}` | ✅ Full | `children` slot | |
| Fragments `<>...</>` | ✅ Full | `Fragment` | |
| Component invocation `<Comp />` | ✅ Full | Function call | |
| `key` prop | ✅ Full | Runtime hint | Used for list diffing |
| `ref` | ✅ Limited | `NodeRef` | Only callback refs |
| `dangerouslySetInnerHTML` | ✅ Full | `dangerous_inner_html` | Escaped by default |
| Custom elements | ⚠️ Partial | Web components | Server-rendered as placeholders |
| SVG | ⚠️ Partial | Inline SVG | Basic shapes only |
| CSS-in-JS | ❌ Excluded | — | Use inline `style` or static CSS |
| Styled-components | ❌ Excluded | — | |

#### JSX Attribute → Rust Mapping

| TSX Attribute | Rust `html!` Attribute | Notes |
|---------------|------------------------|-------|
| `class` / `className` | `class_name` | |
| `style` (object) | `style = {{...}}` | Inline styles only |
| `onClick` | `on_click` | snake_case |
| `onInput` | `on_input` | |
| `onChange` | `on_change` | |
| `onSubmit` | `on_submit` | |
| `onKeyDown` | `on_key_down` | |
| `onFocus` | `on_focus` | |
| `onBlur` | `on_blur` | |
| `dangerouslySetInnerHTML` | `dangerous_inner_html` | |
| `htmlFor` | `html_for` | |

### 2.4 Hooks (Preact API)

| Hook | Status | Rust Equivalent | Notes |
|------|--------|-----------------|-------|
| `useState` | ✅ Full | `use_state` | Returns `(T, Setter<T>)` |
| `useEffect` | ✅ Full | `use_effect` | Deps array supported; cleanup fn supported |
| `useLayoutEffect` | ✅ Full | `use_layout_effect` | Synchronous after DOM mutation |
| `useRef` | ✅ Full | `use_ref` | `RefCell<T>` + `Option<T>` |
| `useCallback` | ✅ Full | `use_callback` | Memoized closure |
| `useMemo` | ✅ Full | `use_memo` | Memoized value |
| `useReducer` | ✅ Full | `use_reducer` | State machine pattern |
| `useContext` | ✅ Full | `use_context` | Context provider + consumer |
| `useId` | ✅ Full | `use_id` | Stable SSR-safe ID |
| `useSignal` | ✅ Full | `use_signal` | Fine-grained reactivity |
| `useComputed` | ✅ Full | `use_computed` | Derived signal |
| `useSyncExternalStore` | ⚠️ Partial | `use_sync_external_store` | Basic store subscription |
| Custom hooks | ✅ Full | Functions returning hook tuples | |

### 2.5 Fresh-Specific APIs

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ Full | `routes/` directory convention |
| Route handlers (`handler` export) | ✅ Full | Object with `GET`/`POST`/`PUT`/`DELETE` |
| `PageProps<T>` | ✅ Full | Generic props with `.data` |
| `HandlerContext` | ✅ Full | `ctx.params`, `ctx.state`, `ctx.render()` |
| Async route components | ✅ Full | `async fn` route default export |
| `routes/_middleware.ts` | ✅ Full | Global middleware |
| `routes/**/_middleware.ts` | ✅ Full | Scoped middleware |
| `routes/**/_layout.tsx` | ✅ Full | Nested layouts with `children` |
| `routes/_404.tsx` / `_500.tsx` | ✅ Full | Error pages |
| `routes/_app.tsx` | ✅ Full | App wrapper |
| Islands (`islands/` directory) | ✅ Full | Partial hydration |
| `IS_BROWSER` | ✅ Full | Compile-time constant |
| `Head` component | ⚠️ Partial | Static `<head>` injection |
| Static files (`static/` dir) | ✅ Full | Served at root |
| Plugin system | ❌ Excluded | Future work |
| Manifest customization | ⚠️ Partial | `runts.config.json` |

### 2.6 Islands & Hydration

| Feature | Status | Notes |
|---------|--------|-------|
| Island components | ✅ Full | `islands/*.tsx` auto-detected |
| `data-island` attribute | ✅ Full | SSR output marker |
| Props serialization | ✅ Full | JSON via `data-props` |
| Hydration strategies | ✅ Full | `eager`, `visible`, `idle`, `manual` |
| `client:load` | ✅ Full | Alias for `eager` |
| `client:idle` | ✅ Full | `requestIdleCallback` |
| `client:visible` | ✅ Full | IntersectionObserver |
| `client:media` | ⚠️ Partial | `matchMedia` query |
| `client:only` | ❌ Excluded | No SSR for island |
| Island inter-island communication | ⚠️ Partial | Shared signals |
| Server islands (server-only rendering) | ⚠️ Partial | No client JS emitted |

### 2.7 Signals (Preact Signals / @preact/signals)

| Feature | Status | Rust Equivalent | Notes |
|---------|--------|-----------------|-------|
| `signal(initial)` | ✅ Full | `Signal::new(initial)` | |
| `computed(fn)` | ✅ Full | `Computed::new(fn)` | |
| `effect(fn)` | ✅ Full | `Effect::new(fn)` | |
| `batch(fn)` | ✅ Full | `batch(fn)` | Transactional updates |
| `untrack(fn)` | ✅ Full | `untrack(fn)` | Read without subscribe |
| Signal in JSX | ✅ Full | Auto-unwrapped | `{signal}` reads `.value` |
| Signal mutation `signal.value = x` | ✅ Full | `signal.set(x)` | Setter via `.set()` in Rust |
| `peek()` | ✅ Full | `signal.peek()` | Read without subscription |

### 2.8 Standard Library APIs

| API | Status | Rust Equivalent |
|-----|--------|-----------------|
| `console.log` | ✅ Full | `println!` |
| `JSON.stringify` | ✅ Full | `serde_json::to_string` |
| `JSON.parse` | ✅ Full | `serde_json::from_str` |
| `Array.map` | ✅ Full | `Iterator::map` |
| `Array.filter` | ✅ Full | `Iterator::filter` |
| `Array.reduce` | ✅ Full | `Iterator::fold` |
| `Array.find` | ✅ Full | `Iterator::find` |
| `Array.includes` | ✅ Full | `Vec::contains` |
| `Array.sort` | ✅ Full | `Vec::sort` / `sort_by` |
| `String.split` | ✅ Full | `String::split` |
| `String.trim` | ✅ Full | `str::trim` |
| `String.startsWith` | ✅ Full | `str::starts_with` |
| `String.replace` | ✅ Full | `str::replace` |
| `Object.keys` | ✅ Full | `HashMap::keys` |
| `Object.entries` | ✅ Full | `HashMap::iter` |
| `Map` / `Set` | ✅ Full | `HashMap` / `HashSet` |
| `Promise` | ✅ Full | `std::future` / `tokio` |
| `fetch` | ✅ Full | `reqwest` / `hyper` | Server-side only |
| `URL` / `URLSearchParams` | ✅ Full | `url` crate |
| `FormData` | ⚠️ Partial | Manual parsing | Server-side |
| `localStorage` | ❌ Excluded | — | Use signals + cookies |
| `sessionStorage` | ❌ Excluded | — | |
| `window` / `document` | ❌ Excluded | — | Use `IS_BROWSER` guards |
| `setTimeout` / `setInterval` | ⚠️ Partial | `tokio::time` | Server only |
| `requestAnimationFrame` | ❌ Excluded | — | Client runtime handles this |
| `addEventListener` | ❌ Excluded | — | Use JSX event handlers |

---

## 3. Explicit Exclusions

These features are **intentionally excluded** and will produce clear compile-time errors:

### 3.1 Language Features

- **`eval()` / `new Function()`** — Requires a JS engine. Use explicit function definitions.
- **`with` statement** — Blocked. Use explicit object access.
- **Labeled statements / `break` to label** — Blocked. Use early returns.
- **`var` declarations** — Blocked. Use `let` or `const`.
- **`for...in` loops** — Blocked. Use `Object.keys()` + `for...of`.
- **`do...while` loops** — Blocked. Use `while` or `for`.
- **`switch` with fall-through** — Blocked. Each case must end with `break`/`return`.
- **Dynamic imports `import()`** — Blocked. Use static imports.
- **`import.meta`** — Blocked. Use `runts.config.json` for build-time config.
- **Decorators `@decorator`** — Blocked. Not stable in TS.
- **Enums (`enum Color { Red }`)** — Blocked. Use `const` objects or discriminated unions.
- **Namespaces / `module` keyword** — Blocked. Use ES modules.
- **Triple-slash directives** — Blocked.
- **`declare` / ambient declarations** — Blocked in user code. Allowed in `.d.ts` for type checking.

### 3.2 React/Preact Features

- **Class components** — Blocked. Use function components.
- **`componentDidMount` etc.** — Blocked. Use `useEffect`.
- **`React.createElement`** — Blocked. Use JSX.
- **`React.Fragment` (explicit)** — Supported via `<>` syntax only.
- **`forwardRef`** — Blocked. Pass refs as props.
- **`memo()`** — Blocked. Components are optimized by default.
- **`Suspense` / `lazy`** — Blocked. Use async components.
- **`ErrorBoundary`** — Blocked. Use `_500.tsx` routes.
- **`Portal`** — Blocked. Use island hydration.
- **`flushSync`** — Blocked. Signals batch automatically.

### 3.3 Browser/DOM APIs

- **`document.getElementById`** — Blocked in components. Use `useRef`.
- **`document.querySelector`** — Blocked in components.
- **`innerHTML` mutation** — Blocked. Use `dangerouslySetInnerHTML` (escaped).
- **`window.addEventListener`** — Blocked. Use JSX event handlers.
- **`navigator` / `location` / `history`** — Blocked. Use Fresh's `useRouter` equivalent.
- **`alert` / `confirm` / `prompt`** — Blocked.
- **`WebSocket` (direct)** — Blocked. Use server-sent events or islands.

### 3.4 Node.js / Deno APIs

- **`process.env`** — Blocked. Use `std::env::var` or config.
- **`fs` module** — Blocked. Use `include_str!` or static files.
- **`path` module** — Blocked. Use Rust `std::path`.
- **`crypto` module** — Blocked. Use Rust `ring` or `rustls`.
- **`Buffer`** — Blocked. Use `Vec<u8>`.

---

## 4. Type System Boundaries

### 4.1 Type Erasure

All TypeScript types are **fully erased at compile time**. Runtime values carry no type information. This matches Rust's zero-cost abstraction model.

```typescript
// TypeScript (compile-time only)
interface User {
  name: string;
  age: number;
}

// Rust (generated) — no runtime type check
struct User {
    name: String,
    age: f64,
}
```

### 4.2 Structural vs Nominal Typing

TypeScript's structural typing is mapped to Rust's nominal typing:

- **Interfaces** → `struct` with derived `Serialize`/`Deserialize`
- **Function types** → `Fn` trait objects (limited) or concrete closures
- **Union types** → Tagged enums with `#[serde(tag = "kind")]`

### 4.3 Null Safety

```typescript
// TypeScript
let name: string | null = null;
name = "hello";

// Rust
let mut name: Option<String> = None;
name = Some("hello".to_string());
```

The compiler inserts `unwrap()` or `?` based on context. In JSX expressions, `Option<T>` auto-maps to empty string for `None`.

---

## 5. Fresh API Compatibility Matrix

| Fresh API | runts Status | Notes |
|-----------|--------------|-------|
| `PageProps` | ✅ | Generic page props with `data`, `url`, `route`, `state` |
| `HandlerContext` | ✅ | `ctx.render()`, `ctx.params`, `ctx.state`, `ctx.next()` |
| `MiddlewareHandler` | ✅ | `(req, ctx) => Response \| ctx.next()` |
| `Handlers` (object) | ✅ | `{ GET, POST }` object export |
| `RouteConfig` | ⚠️ | Partial: `routeOverride` supported |
| `AppProps` | ✅ | App component wrapper |
| `ErrorPageProps` | ✅ | `_404.tsx`, `_500.tsx` |
| `Head` | ⚠️ | Static `<head>` manipulation |
| `asset()` | ✅ | Static file URL generation |
| `defineRoute` | ❌ | Use file-based routing |
| `defineHandler` | ❌ | Use `handler` export |
| `defineLayout` | ❌ | Use `_layout.tsx` |
| `defineApp` | ❌ | Use `_app.tsx` |
| `IS_BROWSER` | ✅ | Compile-time `cfg!(is_browser)` |
| `start(manifest, opts)` | ✅ | Auto-generated in `main.rs` |

---

## 6. Migration Guide: Fresh → runts

### Minimal changes required:

1. **Change imports**: `$fresh/server.ts` → runtime is implicit
2. **Remove Deno-specific APIs**: Replace `Deno.*` with standard APIs
3. **No `import_map.json`**: Use standard ES module paths
4. **Static assets**: Move to `static/` directory
5. **Config**: Convert `fresh.config.ts` → `runts.config.json`

### Before (Fresh):

```tsx
import { Handlers, PageProps } from "$fresh/server.ts";

interface Data {
  message: string;
}

export const handler: Handlers<Data> = {
  async GET(_req, ctx) {
    return ctx.render({ message: "Hello" });
  },
};

export default function Page({ data }: PageProps<Data>) {
  return <h1>{data.message}</h1>;
}
```

### After (runts):

```tsx
interface Data {
  message: string;
}

export const handler = {
  async GET(_req, ctx) {
    return ctx.render({ message: "Hello" });
  },
};

export default function Page({ data }: PageProps<Data>) {
  return <h1>{data.message}</h1>;
}
```

Only import paths change — semantics are identical.

---

## 7. Error Messages

The compiler produces **actionable, Rust-like errors**:

```
error[R0001]: Unsupported language feature
  --> routes/blog/[slug].tsx:42:5
   |
42 |     eval("console.log('x')");
   |     ^^^^ `eval` is not supported in runts
   |
   = help: Use an explicit function definition instead.
   = note: runts compiles to native Rust and cannot execute dynamic code.

error[R0023]: Class components are not supported
  --> islands/OldWidget.tsx:5:1
   |
 5 | class Widget extends Component {
   | ^^^^^ Use function components with hooks instead.

error[R0045]: Missing required prop
  --> routes/index.tsx:12:5
   |
12 |     <Counter />
   |     ^^^^^^^ missing required prop `initial`
   |
   = help: CounterProps requires `initial: number`
```

---

## 8. Versioning

This spec follows semver aligned with the compiler:

- **Patch (0.5.x)**: Bug fixes, better error messages
- **Minor (0.x.0)**: New supported features (expanding the subset)
- **Major (x.0.0)**: Breaking changes to the subset or runtime API

Features are added to the subset based on:
1. Frequency in real Fresh projects (GitHub search)
2. Ability to compile to efficient Rust
3. Maintenance burden of the feature
