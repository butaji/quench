# runts — Complete Design Document

> **Version:** 0.5.1  
> **Date:** 2026-05-27  
> **Status:** MVP Complete → Feature Complete In Progress  
> **Constraint:** Zero external JS runtimes (no V8, no Deno, no WebAssembly JS)

---

## 1. Supported TypeScript/TSX Subset Specification

### 1.1 Philosophy

runts compiles TypeScript/TSX to native Rust. We are **ruthlessly minimal** — every supported feature must have a clean, zero-overhead Rust equivalent. If a pattern cannot compile to efficient native code without a JS runtime, it is excluded.

**Priorities (in order):**
1. Fresh framework compatibility (routes, islands, layouts, middleware, `_app.tsx`)
2. Preact functional components + hooks
3. Fine-grained signals reactivity
4. TypeScript structural types (interfaces, type aliases, unions)
5. Common JS stdlib patterns (Array methods, JSON, Math)

### 1.2 Supported Language Features

| Feature | Syntax | Rust Equivalent | Notes |
|---------|--------|-----------------|-------|
| `const` | `const x = 5` | `let x = 5;` | Immutable binding |
| `let` | `let x = 5` | `let mut x = 5;` | Mutable binding |
| `var` | `var x = 5` | `let mut x = 5;` | Hoisting flattened; treated as `let` |
| Destructuring (object) | `const {a, b} = obj` | `let a = obj.a; let b = obj.b;` | Nested supported |
| Destructuring (array) | `const [a, b] = arr` | `let a = arr[0]; let b = arr[1];` | Rest patterns supported |
| Rest in destructuring | `const {a, ...rest} = obj` | Named struct + HashMap capture | |
| Arrow functions | `const f = (x) => x` | `let f = \|x\| x;` | Multi-statement bodies supported |
| Async/await | `async function f()` | `pub async fn f()` | Top-level and nested |
| Template literals | `` `hello ${name}` `` | `format!("hello {}", name)` | Expressions interpolated |
| Optional chaining | `obj?.prop` | `obj.as_ref().map(\|o\| o.prop)` | |
| Nullish coalescing | `a ?? b` | `a.unwrap_or(b)` | |
| Spread operator | `{...obj, a: 1}` | Struct update syntax or `HashMap::extend` | Arrays: `vec!` concatenation |
| Ternary | `cond ? a : b` | `if cond { a } else { b }` | |
| Logical operators | `&&`, `\|\|` | `&&`, `\|\|` | Short-circuit preserved |
| Comparison | `===`, `!==` | `==`, `!=` | Strict equality mapped to Rust equality |
| `for` loops | `for (let i=0; i<n; i++)` | `for i in 0..n` | Range-based only |
| `for...of` | `for (const x of arr)` | `for x in arr` | |
| `if`/`else` | Standard | Standard | |
| `switch` | `switch (x) { case a: ... }` | `match x { a => ... }` | Exhaustive by default |

### 1.3 Supported JSX/TSX

| Feature | Status | Notes |
|---------|--------|-------|
| Element creation | ✅ Full | `<div>content</div>` → `html!(<div>"content"</div>)` |
| Self-closing | ✅ Full | `<img />` |
| Fragments | ✅ Full | `<></>` → `html!(<></>)` |
| Component usage | ✅ Full | `<Counter initial={5} />` |
| Expressions in children | ✅ Full | `{count}` |
| Expressions in attributes | ✅ Full | `className={active ? "on" : "off"}` |
| Spread attributes | ✅ Full | `<div {...props} />` |
| Event handlers | ✅ Full | `onClick={handler}` → `on_click = {handler}` |
| `key` attribute | ✅ Full | Used for list rendering |
| `dangerouslySetInnerHTML` | ❌ Excluded | Security risk; not needed for SSR |

### 1.4 Supported Preact Hooks

| Hook | Status | Notes |
|------|--------|-------|
| `useState` | ✅ Full | With lazy init variant |
| `useEffect` | ✅ Full | Cleanup supported; flushed after render |
| `useLayoutEffect` | ✅ Stub | SSR no-op; would run sync on client |
| `useRef` | ✅ Full | `useRef<T>(null)` pattern |
| `useMemo` | ✅ Full | Dependency-hash based memoization |
| `useCallback` | ✅ Full | Function memoization |
| `useReducer` | ✅ Full | Full reducer pattern |
| `useContext` | ✅ Full | `createContext` + `useContext` |
| `useId` | ✅ Full | Stable unique IDs (`rts-{n}`) |
| `useSyncExternalStore` | ✅ Full | SSR snapshot variant |
| `useErrorBoundary` | ✅ Full | Error state + reset callback |
| `useSignal` | ✅ Full | Preact Signals → `Signal::new` |
| `useComputed` | ✅ Full | Derived signals |
| `useSignalEffect` | ✅ Full | Signal-based effects |

### 1.5 Supported Fresh-Specific Features

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ Full | Static, param `[id]`, catch-all `[...path]` |
| Route groups | ✅ Full | `(group)/` ignored in URL |
| Layouts | ✅ Full | `_layout.tsx` with nested composition |
| `_app.tsx` | ✅ Full | Root wrapper component with `children` prop |
| Middleware | ✅ Full | `_middleware.ts` with `ctx.next()` |
| Islands | ✅ Full | `islands/` directory, partial hydration |
| `PageProps` | ✅ Full | Typed params + data |
| `HandlerContext` | ✅ Full | `ctx.render()`, `ctx.state` |
| `Handler` export | ✅ Full | Object with GET/POST/PUT/DELETE |
| Error pages | ✅ Full | `_404.tsx`, `_500.tsx` with defaults |
| `State` | ✅ Full | Middleware state typing |

### 1.6 Explicitly Excluded Features

**Language Features:**
- `eval()`, `new Function()` — no runtime code generation
- `with` statement — scoping nightmare
- `try/catch` — handled via Rust `Result`; limited support
- Prototype manipulation (`__proto__`, `Object.setPrototypeOf`)
- Dynamic property access (`obj[dynamicKey]`) — must be statically analyzable
- `Symbol`, `Reflect`, `Proxy` — meta-programming requires JS runtime
- `instanceof` with custom constructors
- Generators (`function*`) and `yield`
- `for...in` loops over object keys

**React/Preact Patterns:**
- Class components — functional components only
- `createRef()` — use `useRef` instead
- `forwardRef()` — simplified ref forwarding via props
- `useImperativeHandle` — not applicable without class components
- `useLayoutEffect` — stub only; full implementation deferred
- Context providers as classes
- `React.Children` API — use arrays directly

**Type System:**
- `any` — forbidden; use explicit unions
- `unknown` — partially supported
- Conditional types (`T extends U ? X : Y`) — deferred
- Template literal types — excluded
- `infer` keyword — excluded
- `declare` — no ambient declarations
- Namespace/module augmentation — excluded

---

## 2. Architecture & Transpilation Strategy

### 2.1 High-Level Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         runts Architecture                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  User Code (TS/TSX)                                                          │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ routes/  islands/  components/  middleware/  lib/  static/          │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                       │
│                                      ▼                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Transpiler Pipeline                               │   │
│  │  ┌──────────┐   ┌─────────────┐   ┌─────────────────────────────┐  │   │
│  │  │  Parser  │──▶│  Analyzer   │──▶│   Rust Code Generator       │  │   │
│  │  │  (TSX)   │   │ (Semantic)  │   │   (In-Memory)               │  │   │
│  │  └──────────┘   └─────────────┘   └─────────────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                       │
│              ┌───────────────────────┴───────────────────────┐               │
│              │                                               │               │
│              ▼                                               ▼               │
│  ┌─────────────────────┐                         ┌─────────────────────┐   │
│  │   Development Mode   │                         │   Production Mode   │   │
│  │  rquickjs (TSX→JS)   │                         │  Cargo Build         │   │
│  │  < 50ms hot reload   │                         │  --release binary    │   │
│  │  Axum dev server     │                         │  ~1.5MB binary       │   │
│  └─────────────────────┘                         └─────────────────────┘   │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        Runtime Stack                                 │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐   │   │
│  │  │  VDOM    │  │  Signals │  │  Hooks   │  │  Islands (SSR+   │   │   │
│  │  │  VNode   │  │  Leptos  │  │  Indexed │  │   Hydration)     │   │   │
│  │  │  → HTML  │  │  style   │  │  array   │  │                  │   │   │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Parser (TSX → HIR)

- **Recursive descent parser**, zero external dependencies
- Handles full supported subset: TS types, JSX, async/await, destructuring
- Produces `Module` → `Decl` → `Stmt` → `Expr` tree
- JSX is parsed into `Expr::JSX(JSXElement)` nodes
- Source positions preserved for error reporting

**Key design decisions:**
- No separate lexer; tokenization is inline
- Type annotations parsed but immediately converted to type hints
- JSX attributes normalized to camelCase → snake_case in codegen

### 2.3 HIR (High-Level IR)

The HIR is a normalized, typed AST:

```rust
pub struct Module {
    pub items: Vec<ModuleItem>,
}

pub enum ModuleItem {
    Import(ImportDecl),
    Export(Export),
    Decl(Decl),
}

pub enum Decl {
    Function(FunctionDecl),
    Variable(VarDecl),
    Type(TypeDecl),
}

pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Option<Block>,
    pub is_async: bool,
}
```

**Properties:**
- All types are explicit (inferred types annotated)
- JSX preserved as-is (transformed in codegen)
- All statements flattened to basic control flow

### 2.4 Semantic Analyzer

- **Island detection**: Files in `islands/` are marked for client hydration
- **Route detection**: Files in `routes/` are classified by pattern
- **Hook validation**: Ensures hooks are called at top level, in consistent order
- **Type inference**: Maps TS types to Rust types (`string` → `String`, `number` → `f64`)
- **Middleware extraction**: Identifies `_middleware.ts` files

### 2.5 Rust Code Generation

**TS/TSX → Rust Transform Rules:**

| TypeScript | Rust |
|-----------|------|
| `function name() {}` | `pub fn name() {}` |
| `async function` | `pub async fn` |
| `const x = 5` | `let x = 5.0` |
| `let arr = [1, 2]` | `let arr = vec![1.0, 2.0]` |
| `obj.prop` | `obj.prop` (snake_case auto-converted) |
| `arr[0]` | `arr[0usize]` |
| `arr.slice(1, 3)` | `arr[1..3].to_vec()` |
| `str.length` | `str.len()` |
| `str.toLowerCase()` | `str.to_lowercase()` |
| `arr.map(fn)` | `arr.iter().map(fn).collect::<Vec<_>>()` |
| `arr.filter(fn)` | `arr.iter().filter(fn).collect::<Vec<_>>()` |
| `new Response(body, init)` | `Response::builder()...body(...).unwrap()` |
| `Date.now()` | `std::time::SystemTime::now()` |
| `console.log(...)` | `tracing::info!(...)` |

**JSX → `html!` Macro:**

```tsx
// Source
<div className="hero" style={{ padding: "1rem" }}>
  <h1>{title}</h1>
  <Counter initial={5} />
</div>
```

```rust
// Generated
html!(<div class = "hero" style = "padding: 1rem">
  <h1>{title.clone()}</h1>
  <Counter initial = 5 />
</div>)
```

**Islands Codegen:**
- Islands are generated as both Rust components (SSR) and JS modules (client)
- SSR renders `<div data-island="Name" data-props="{json}">...</div>`
- Client JS runtime hydrates these placeholders

### 2.6 Runtime Architecture

**Production Server Stack:**
```
Axum Router
    ├── Tower middleware (compression, CORS, tracing)
    ├── Route handlers (generated from routes/)
    │   ├── Middleware pipeline
    │   ├── Handler execution
    │   └── Component SSR (VNode → HTML)
    ├── Island bundle endpoint (/_runts/islands/:name)
    └── Static file serving
```

**Signal System (Fine-Grained Reactivity):**

| Preact Signal | Rust Equivalent |
|---------------|-----------------|
| `signal(initial)` | `Signal::new(initial)` |
| `computed(fn)` | `Computed::new(fn)` |
| `effect(fn)` | `Effect::new(fn)` |
| `batch(fn)` | `batch(fn)` — defers effect triggering |
| `untrack(fn)` | `untrack(fn)` — disables subscription |

Implementation uses dependency tracking: effects subscribe to signals they read; signals notify subscribers on change.

**Islands Architecture & Partial Hydration:**

```
Server SSR:
  1. Render island component to HTML
  2. Wrap in <div data-island="Name" data-props="{json}">
  3. Include island JS bundle reference

Client Hydration:
  1. Parse data-island markers
  2. Load island JS module
  3. Instantiate component with deserialized props
  4. Attach event listeners
  5. Activate signal effects
```

Hydration strategies: `load` (immediate), `visible` (IntersectionObserver), `idle` (requestIdleCallback), `interaction` (on first click/hover).

### 2.7 Development Mode: rquickjs (HIR Interpreter REMOVED)

**Core principle:** TSX → JS bundle → rquickjs eval. No Rust compilation in dev.

**Execution flow:**
1. File watcher detects changes
2. Re-parse changed file → oxc_ast
3. Transpile to JS bundle (`oxc_codegen`: JSX → `React.createElement`, TS erased)
4. Create rquickjs context + inject Yoga bridge globals
5. Eval JS bundle → VNode tree → Yoga layout → render

**rquickjs capabilities:**
- Full ES2020 expression evaluation (arithmetic, logical, string ops)
- Real React hooks (`useState`, `useEffect`, `useMemo`, etc.)
- JSX evaluation via `React.createElement` → bridge → VNode
- Component rendering with full hook state
- Crossterm event routing (`useInput`, `useApp`, etc.)

**Hot reload latency:** < 100ms (typically ~50ms for single-file changes)

### 2.8 Production Mode: Native Compilation

**Build pipeline:**
1. Parse all TS/TSX → HIR (parallel via rayon)
2. Generate Rust source files (`src/gen/*.rs`)
3. Generate route table (`src/routes.rs`)
4. Generate islands manifest (`src/islands.rs`)
5. Run `cargo build --release`
6. Strip + optimize binary

**Generated Rust structure:**
```
src/
├── main.rs              # Server entry (Axum)
├── lib.rs               # Module re-exports
├── routes.rs            # Route table (generated)
├── islands.rs           # Islands manifest (generated)
├── components.rs        # Component re-exports
└── gen/
    ├── index.rs         # Route: /
    ├── about.rs         # Route: /about
    ├── blog/
    │   ├── mod.rs
    │   ├── index.rs     # Route: /blog
    │   └── slug.rs      # Route: /blog/:slug
    ├── islands/
    │   ├── counter.rs
    │   └── todo_list.rs
    └── components/
        └── header.rs
```

**Incremental builds:** SHA-256 content hash cache in `.runts/cache/build_cache.json`. Only changed files are re-transpiled.

---

## 3. Roadmap: MVP → Full Fresh Coverage

### Phase 0: Foundation ✅ COMPLETE (v0.5.0)

| Milestone | Status |
|-----------|--------|
| Custom TSX parser (recursive descent) | ✅ |
| HIR (High-Level IR) | ✅ |
| Semantic analyzer | ✅ |
| Rust code generator | ✅ |
| rquickjs dev engine | ✅ | TSX→JS→rquickjs. HIR interpreter removed. |
| Dev server (Axum + file watcher) | ✅ |
| Signal system | ✅ |
| Hooks engine (8 hooks) | ✅ |
| VDOM / SSR renderer | ✅ |
| Islands architecture | ✅ |
| Client runtime (vanilla JS) | ✅ |
| File-based routing | ✅ |
| Middleware pipeline | ✅ |
| Route handlers | ✅ |
| `_app.tsx` wrapper | ✅ |
| Route groups `(group)/` | ✅ |
| `useErrorBoundary` | ✅ |
| Project scaffolding (`runts init`) | ✅ |
| Example project (`my-blog`) | ✅ |

### Phase 1: Feature Complete (v0.6.0) — Current Focus

| Feature | Priority | Status |
|---------|----------|--------|
| `_500.tsx` / `_error.tsx` error boundaries | P1 | ✅ |
| API routes (`routes/api/*.ts`) | P2 | 🔄 |
| Plugin system (build hooks) | P2 | ❌ |
| Source maps | P2 | ❌ |
| `_app.tsx` root wrapper (full component render) | P1 | ✅ |
| Route groups | P2 | ✅ |

### Phase 2: Production Hardening (v0.7.0)

- Binary size optimization (< 1.5MB target)
- Startup time optimization (< 5ms)
- Memory usage optimization (< 3MB RSS)
- Load testing and benchmarking suite
- Security audit (XSS prevention, header validation)

### Phase 3: Developer Experience (v0.8.0)

- Better error recovery (parser continues after first error)
- IDE integration (LSP for .tsx files)
- VS Code extension
- Debug source maps
- Hot reload WebSocket (no page refresh)

### Phase 4: Ecosystem Integration (v0.9.0)

- npm-compatible package manager integration
- Third-party island marketplace
- Database integration helpers (ORM-agnostic)
- Deployment templates (Docker, Fly.io, Railway)

### Phase 5: Stable (v1.0.0)

- 98%+ Fresh starter project compatibility
- Stable API guarantee
- Comprehensive documentation
- Commercial support options

---

## 4. Performance Targets & Trade-offs

### 4.1 Production Binary (Current: v0.5.1)

| Metric | Target | v0.5.1 Status | Methodology |
|--------|--------|---------------|-------------|
| **Binary size** | < 2 MB | **~1.5 MB** | `cargo build --release`, `strip`, `du -h` |
| **Memory (baseline RSS)** | < 3 MB | ~2.8 MB | `ps -o rss= -p <pid>` at idle |
| **Cold start** | < 5 ms | < 10 ms | Time from process start to first HTTP response |
| **Hot request latency (p50)** | < 0.5 ms | < 1 ms | SSR of a simple page, warmed up |
| **Hot request latency (p99)** | < 2 ms | < 5 ms | Same, 99th percentile |
| **SSR throughput** | > 50k req/s | ~15k req/s | `wrk -t12 -c400 -d30s` on simple page |
| **Max concurrent connections** | > 10k | > 10k | Limited by OS file descriptors |

### 4.2 Development Mode

| Metric | Target | v0.5.1 Status |
|--------|--------|---------------|
| **Hot reload latency** | < 50 ms | **< 20 ms** |
| **Initial dev server start** | < 3 s | **< 2 s** |
| **HIR parse speed** | > 1k files/s | ~2k files/s |
| **rquickjs eval overhead** | ~50ms startup | ~50ms |

### 4.3 Client Runtime (Browser)

| Metric | Target | Notes |
|--------|--------|-------|
| **Island JS bundle size** | < 12 KB (gzipped) | Runtime + signals + hydration |
| **Time to interactive (island)** | < 50 ms | From HTML parse to event listeners attached |
| **Hydration overhead** | < 5 ms per island | Deserialize props + attach listeners |

### 4.4 Trade-offs

| Decision | Rationale | Cost |
|----------|-----------|------|
| **Custom parser instead of swc** | Zero deps, fast compile, small binary | Parser maintenance burden; subset limitation |
| **rquickjs in dev** | Full JS semantics, ~1MB overhead | Slightly slower than HIR interpreter; correct JS semantics |
| **String-based codegen** | Simple, debuggable, fast | Not using `syn` AST means no structured manipulation |
| **TypeScript types fully erased** | Clean Rust codegen; no runtime overhead | No runtime type validation; `any` is forbidden |
| **No V8/WASM JS** | Smallest binary, fastest cold start | Cannot run arbitrary JS libraries; subset required |
| **Functional components only** | Simpler codegen; aligns with modern React | No class component compatibility |
| **Island hydration (not full app)** | Minimal JS sent to client; fast TTI | Islands cannot easily share non-signal state |

---

## 5. Verification

### 5.1 Test Coverage (106 passing)

- `runts-ink`: 59 unit tests passing (bridge props, hooks, VNode serialization)
- `runts-plugin`: 5 tests passing (typed boundary)
- Compile path: `tests/compile_path.rs` with 5 ignored integration tests
- Transpile tests: 864 tests passing, 0 failures, 99 ignored (all 15 modules enabled)
- Analyzer: Island detection, route patterns, hook validation
- Codegen: All TS patterns → Rust, JSX → html!
- Routes: Pattern matching, parameter extraction, route groups
- Incremental: Cache save/load, hash computation, pruning

### 5.2 Build Verification

```bash
cargo test -p runts-ink     # 59 tests pass
cargo test -p runts-plugin  # 5 tests pass
cargo check                 # Clean (56 warnings from dead code)
cargo build --release       # ~1.5MB binary
./target/release/my-blog    # Serves on :8000
```

### 5.3 Example Blog End-to-End

The `examples/my-blog` project demonstrates:
- File-based routing (`/`, `/about`, `/blog`, `/blog/:slug`)
- Islands (`Counter` with `useState`, `TodoList`)
- Layouts (`blog/_layout.tsx`)
- Middleware (`_middleware.ts`)
- `_app.tsx` root wrapper
- Route groups

---

*Document version: 0.5.1 | Last updated: 2026-05-27*
