# runts Supported TypeScript/TSX Subset Specification

> **Version:** 0.5.0  
> **Goal:** Cover 95%+ of real-world Fresh/Preact usage with a minimal, compilable subset.

---

## 1. Philosophy

runts compiles TypeScript/TSX to native Rust. We are **ruthlessly minimal** — every supported feature must have a clean, zero-overhead Rust equivalent. If a pattern cannot compile to efficient native code without a JS runtime, it is excluded.

**Priorities (in order):**
1. Fresh framework compatibility (routes, islands, layouts, middleware)
2. Preact functional components + hooks
3. Fine-grained signals reactivity
4. TypeScript structural types (interfaces, type aliases, unions)
5. Common JS stdlib patterns (Array methods, JSON, Math)

---

## 2. Supported Features

### 2.1 Type System

| Feature | Status | Notes |
|---------|--------|-------|
| Primitive types (`string`, `number`, `boolean`, `null`, `undefined`) | ✅ Full | Direct Rust mappings |
| Interfaces | ✅ Full | → `struct` with `Serialize/Deserialize` |
| Type aliases | ✅ Full | → `type` or `struct` |
| Object types (inline) | ✅ Full | → anonymous struct or `HashMap` |
| Array types (`T[]`, `Array<T>`) | ✅ Full | → `Vec<T>` |
| Tuple types | ✅ Full | → Rust tuples |
| Union types (`A \| B`) | ✅ Partial | Simple unions → `enum`; nullables → `Option<T>` |
| Generics (`<T>`, `<T extends U>`) | ✅ Partial | Monomorphized at codegen time |
| `Record<K, V>` | ✅ Full | → `HashMap<K, V>` |
| Function types | ✅ Partial | → `Box<dyn Fn(...)>` |
| Mapped/Conditional types | ❌ Excluded | Use explicit unions |
| `any` / `unknown` | ⚠️ Fallback | → `serde_json::Value` |

### 2.2 JSX / TSX

| Feature | Status | Rust Target |
|---------|--------|-------------|
| Element syntax (`<div />`) | ✅ Full | `html!` macro / `VNode::element` |
| Components (`<Component />`) | ✅ Full | Function call returning `VNode` |
| Attributes (`class="x"`) | ✅ Full | Struct fields / `HashMap` |
| Spread attributes (`{...props}`) | ✅ Full | `HashMap` merge |
| Boolean attributes (`disabled`) | ✅ Full | `AttrValue::Bool` |
| Inline styles (`style={{ color: "red" }}`) | ✅ Full | CSS string conversion |
| Fragments (`<>...</>`) | ✅ Full | `VNode::Fragment` |
| Children (text, expr, nested JSX) | ✅ Full | `Vec<VNode>` |
| Conditional rendering (`{cond && <X />}`) | ✅ Full | `if cond { Some(...) }` |
| List rendering (`{items.map(...)}`) | ✅ Full | `.iter().map(...).collect()` |
| Dynamic tags (`<Tag />`) | ⚠️ Deferred | v0.6 — requires runtime dispatch |
| Event handlers (`onClick={fn}`) | ✅ SSR | Stripped from SSR; reattached client-side |

### 2.3 Expressions & Statements

| Feature | Status | Notes |
|---------|--------|-------|
| Variables (`const`, `let`, `var`) | ✅ Full | `let` / `let mut` |
| Destructuring (`{a, b}`, `[x, y]`) | ✅ Full | Multi-statement `let` bindings |
| Arrow functions | ✅ Full | Closures / `\|x\| ...` |
| Function declarations | ✅ Full | `fn` |
| `if` / `else` | ✅ Full | Native Rust `if` |
| Ternary (`a ? b : c`) | ✅ Full | Native Rust `if` |
| `for` loops | ✅ Full | `for` / `while` |
| `for...of` / `for...in` | ✅ Full | `.iter()` |
| `while` | ✅ Full | Native |
| `switch` | ✅ Full | `match` |
| `try` / `catch` | ✅ Partial | `Result` ergonomics |
| `throw` | ✅ Partial | `panic!` / `Result::Err` |
| Template literals | ✅ Full | `format!` |
| Spread (`...args`) | ✅ Partial | `.clone()` / `.extend()` |
| Optional chaining (`?.`) | ⚠️ Simplified | `Option` unwrapping |
| Nullish coalescing (`??`) | ✅ Full | `.unwrap_or(...)` |

### 2.4 Preact Hooks

| Hook | Status | Rust Implementation |
|------|--------|---------------------|
| `useState` | ✅ Full | `use_state<T>()` → `(T, impl Fn(T))` |
| `useEffect` | ✅ Full | `use_effect<F, D>()` — batched on client |
| `useRef` | ✅ Full | `use_ref<T, F>()` → `Ref<T>` |
| `useMemo` | ✅ Full | Memoized closure with dep hashing |
| `useCallback` | ✅ Full | Same as `useMemo` for functions |
| `useReducer` | ✅ Full | `use_reducer<S, A, R>()` |
| `useContext` | ✅ Full | `Context<T>` with downcast |
| `useId` | ✅ Full | Atomic counter |
| `useSyncExternalStore` | ✅ Full | Server snapshot fallback |

### 2.5 Preact Signals

| API | Status | Rust Implementation |
|-----|--------|---------------------|
| `signal(initial)` / `useSignal` | ✅ Full | `Signal<T>` (RwLock + subscriber list) |
| `computed(fn)` / `useComputed` | ✅ Full | `Computed<T>` with dependency tracking |
| `effect(fn)` / `useSignalEffect` | ✅ Full | `Effect` with auto-subscription |
| `batch(fn)` | ✅ Full | Batched updates (no-op in current tick) |
| `untrack(fn)` | ✅ Full | Disables tracking scope |

### 2.6 Fresh-Specific

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ Full | `routes/` → Axum router |
| Dynamic routes (`[slug].tsx`) | ✅ Full | Regex capture groups |
| Catch-all (`[...path].tsx`) | ✅ Full | `(?P<path>.*)` |
| Layouts (`_layout.tsx`) | ✅ Full | Nested composition chain |
| Middleware (`_middleware.ts`) | ✅ Full | Pipeline with `ctx.next()` |
| Islands (`islands/`) | ✅ Full | SSR + hydration manifest |
| `PageProps` | ✅ Full | Typed route params |
| `HandlerContext` | ✅ Full | Request + state + render helpers |
| `Handler` export | ✅ Full | Object with GET/POST/etc methods |
| `Default` export | ✅ Full | Page component |
| Error pages (`_404.tsx`, `_500.tsx`) | ✅ Full | Fallback chain |
| `_app.tsx` wrapper | ⚠️ Partial | v0.6 — root app component |

---

## 3. Explicitly Excluded

These features are **intentionally unsupported** because they require a JS runtime, dynamic evaluation, or produce unbounded codegen complexity.

### 3.1 Language Features

| Feature | Reason | Workaround |
|---------|--------|------------|
| `eval()` / `new Function()` | Requires JS engine | N/A — redesign logic |
| `with` statement | Not representable in Rust | Destructure explicitly |
| Dynamic `import()` | Requires module loader | Static imports only |
| `require()` / CommonJS | Module system mismatch | ES modules only |
| `Symbol` / `Symbol.iterator` | Runtime complexity | Use `Vec` / `HashMap` |
| Generators (`function*`) | State machine complexity | Use `Iterator` or async |
| `Proxy` | Runtime interception | Explicit wrapper types |
| `Reflect` | Meta-programming | Direct method calls |

### 3.2 React/Preact Patterns

| Feature | Reason | Workaround |
|---------|--------|------------|
| Class components | Lifecycle complexity | Functional + hooks |
| `React.memo` | Shallow comparison overhead | Manual `useMemo` |
| `React.forwardRef` | Ref forwarding complexity | Direct prop passing |
| `React.lazy` / `Suspense` | Dynamic imports | Static route splitting |
| `createPortal` | DOM manipulation | Conditional rendering |
| Error boundaries | Try/catch in render | v0.7 — `Result`-based |

### 3.3 Type System

| Feature | Reason | Workaround |
|---------|--------|------------|
| Conditional types (`T extends U ? X : Y`) | Infinite expansion | Explicit overloads |
| Recursive types | Unbounded codegen | Base-case explicit types |
| Namespace merging | Module complexity | Use modules/files |
| Decorators | Stage-2, metadata reflection | Plain functions |
| `infer` keyword | Type extraction | Explicit type params |

---

## 4. Stdlib Mappings

Native JavaScript APIs compile to Rust equivalents:

| JS API | Rust Target |
|--------|-------------|
| `JSON.stringify` | `serde_json::to_string` |
| `JSON.parse` | `serde_json::from_str` |
| `Array.map` | `.iter().map(...).collect::<Vec<_>>()` |
| `Array.filter` | `.iter().filter(...).cloned().collect()` |
| `Array.reduce` | `.iter().fold(...)` |
| `Array.find` | `.iter().find(...)` |
| `Array.includes` | `.contains(...)` |
| `Array.join` | `.join(...)` |
| `Array.push` | `.push(...)` |
| `Array.sort` | `.sort_by(...)` |
| `Object.keys` | `.keys().collect::<Vec<_>>()` |
| `Object.entries` | `.iter().collect::<Vec<_>>()` |
| `Math.random` | `rand::random::<f64>()` |
| `Math.floor/ceil/round` | `.floor()` / `.ceil()` / `.round()` |
| `Date.now()` | `SystemTime::now().duration_since(UNIX_EPOCH)` |
| `console.log/warn/error` | `tracing::info/warn/error` |

---

## 5. Migration Guide: Fresh → runts

**Files that work with zero changes:**
- Simple functional components
- `useState`, `useEffect`, `useRef`, `useMemo`
- `useSignal`, `useComputed`
- File-based routes with `export default`
- Handler objects with `GET`/`POST`
- Layouts with `children` prop

**Changes required:**
- Replace `useId` import from `preact` with runtime built-in
- Remove `React.memo` wrappers
- Replace dynamic `import()` with static imports
- Convert class components to functional components
- Replace `try/catch` in JSX with `Result`-based patterns where possible

---

## 6. Verification Matrix

| Fresh Example Project | Compatibility |
|-----------------------|---------------|
| `fresh-project` (default starter) | ✅ 100% |
| `fresh-blog` (markdown routes) | ✅ 95% (needs custom markdown loader) |
| `fresh-deno-kv` (DB routes) | ✅ 90% (replace Deno.KV with Postgres/Redis) |
| `fresh-todo` (island interactivity) | ✅ 100% |
| `fresh-tw` (Tailwind) | ✅ 100% (Tailwind is build-time) |

