# runts — Design Document

> **Status:** v0.5.0 — MVP complete. All core subsystems implemented, tested, and producing working binaries.
> **Last updated:** 2026-05-27

---

## 1. Supported TypeScript/TSX Subset

### 1.1 Philosophy

runts compiles a **minimal but sufficient** subset of TypeScript + TSX that covers **>95% of real-world Fresh/Preact** usage. We are ruthless about exclusions: if a feature adds parser complexity without enabling common Fresh patterns, it is rejected.

**Key principle:** *If it compiles in runts, it compiles to native Rust with zero JS runtime.*

### 1.2 Supported Features

#### Type System (compile-time only; erased at runtime)
| Feature | Status | Notes |
|---------|--------|-------|
| Primitive types (`string`, `number`, `boolean`) | ✅ | `number` maps to `f64` |
| `Array<T>` / `T[]` | ✅ | Maps to `Vec<T>` |
| `Record<K, V>` | ✅ | Maps to `HashMap<K, V>` |
| Interfaces | ✅ | Maps to `#[derive(Serialize, Deserialize)]` structs |
| Type aliases | ✅ | Erased; used for struct naming |
| Optional properties (`?`) | ✅ | Maps to `Option<T>` |
| Union types (`A | B`) | ✅ | Simple unions only; complex unions → `serde_json::Value` |
| Generics (`<T>`) | ✅ | Basic generic structs/functions |
| `any` / `unknown` | ✅ | Maps to `serde_json::Value` |
| `void` | ✅ | Maps to `()` |
| `null` / `undefined` | ✅ | Maps to `Option<T>` or `()` |

#### JSX / TSX
| Feature | Status | Notes |
|---------|--------|-------|
| HTML elements | ✅ | Transformed to `html!` macro calls |
| Components (PascalCase) | ✅ | Resolved via component registry at render time |
| Self-closing tags | ✅ | |
| Fragments (`<>...</>`) | ✅ | Maps to `VNode::Fragment` |
| Spread attributes (`{...props}`) | ✅ | |
| `className` → `class` | ✅ | Auto-mapped |
| Inline `style` objects | ✅ | Converted to CSS strings |
| `key` attribute | ✅ | Passed through to VNode |
| Event handlers (`onClick`) | ✅ | Stripped in SSR; shipped for island hydration |
| Dynamic tags | ⚠️ | Limited; component names must be known at compile time |

#### Expressions & Statements
| Feature | Status | Notes |
|---------|--------|-------|
| Variables (`const` / `let`) | ✅ | `var` normalized to `let` |
| Functions (named, async, arrow) | ✅ | |
| Destructuring (`{a, b}`, `[x, y]`) | ✅ | Expanded to individual bindings in codegen |
| Template literals | ✅ | Converted to `format!()` |
| Ternary (`cond ? a : b`) | ✅ | |
| `if` / `else` | ✅ | |
| `for` / `while` | ✅ | |
| `return` | ✅ | |
| `await` | ✅ | |
| `try` / `catch` | ✅ | Maps to Rust `Result` handling |
| `throw` | ✅ | Maps to `panic!` |
| Array methods (`.map`, `.filter`, `.reduce`, `.find`, `.includes`, `.slice`) | ✅ | Translated to Rust iterator methods |
| String methods (`.split`, `.trim`, `.startsWith`, `.replace`, `.toLowerCase`) | ✅ | Translated to Rust `String` methods |
| `JSON.parse` / `JSON.stringify` | ✅ | Maps to `serde_json` |
| `Date.now()` | ✅ | Maps to `std::time::SystemTime` |
| `Math.*` | ✅ | Maps to `f64` methods or `std::f64::consts` |
| `console.*` | ✅ | Maps to `tracing` |

#### Preact Hooks (full compatibility)
| Hook | Status | Rust equivalent |
|------|--------|-----------------|
| `useState` | ✅ | Indexed hook state (`use_state`) |
| `useEffect` | ✅ | Queued effects (`use_effect`) |
| `useRef` | ✅ | `Arc<RwLock<T>>` wrapper (`use_ref`) |
| `useMemo` | ✅ | Hash-based memoization (`use_memo`) |
| `useCallback` | ✅ | Same as `useMemo` for closures |
| `useReducer` | ✅ | State + dispatch closure (`use_reducer`) |
| `useContext` | ✅ | Thread-local context store |
| `useId` | ✅ | Atomic counter |
| `useSyncExternalStore` | ✅ | Server snapshot fallback |
| `useErrorBoundary` | ✅ | Error state + reset callback |

#### Preact Signals
| Feature | Status | Rust equivalent |
|---------|--------|-----------------|
| `signal(initial)` | ✅ | `Signal::new(initial)` |
| `computed(fn)` | ✅ | `Computed::new(fn)` |
| `effect(fn)` | ✅ | `Effect::new(fn)` |
| `batch(fn)` | ✅ | `batch(fn)` — defers effect triggering |
| `untrack(fn)` | ✅ | `untrack(fn)` — disables subscription |

#### Fresh-Specific
| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ | `routes/` → Axum router |
| Dynamic routes (`[id].tsx`) | ✅ | Regex-based param extraction |
| Nested routes (`blog/[slug].tsx`) | ✅ | |
| Catch-all (`[...slug].tsx`) | ✅ | |
| Route groups (`(group)/`) | ✅ | Ignored in URL path |
| Route handlers (`GET` / `POST` / `PUT` / `DELETE`) | ✅ | |
| `ctx.render(data)` | ✅ | SSR with props injection |
| `ctx.params` | ✅ | HashMap access |
| Middleware (`_middleware.ts`) | ✅ | Pipeline execution |
| Islands (`islands/`) | ✅ | Selective hydration |
| Hydration strategies (`load`, `idle`, `visible`, `interaction`) | ✅ | |
| Layouts (`_layout.tsx`) | ✅ | Nested composition |
| `_app.tsx` root wrapper | ✅ | Full component render with `children` |
| Error pages (`_404.tsx`, `_500.tsx`) | ✅ | |
| Static files (`static/`) | ✅ | Served by dev + production |
| `IS_BROWSER` constant | ✅ | Compile-time flag |

### 1.3 Explicitly Excluded

#### Language Features
| Feature | Rationale |
|---------|-----------|
| `eval()` / `new Function()` | Requires JS runtime |
| `with` statement | Blocked by design |
| `prototype` mutation | Not supported; use plain functions |
| `Symbol` | No Rust equivalent |
| `WeakMap` / `WeakRef` | No Rust equivalent |
| `Proxy` | Too complex; use explicit getters |
| `Generator` / `yield` | Limited async support covers 95% |
| `class` / `extends` | Composition over inheritance; use functions |
| `enum` | Use union types or string literals |
| `namespace` / `module` | Use ES modules |
| `declare` / `/// <reference>` | Purely compile-time; ignored |
| Decorators | No stable JS equivalent |

#### React/Preact Patterns
| Pattern | Rationale |
|---------|-----------|
| `React.createElement` | JSX is first-class in HIR |
| `ReactDOM.render` | Not applicable (SSR/native) |
| `dangerouslySetInnerHTML` | XSS risk; use explicit HTML macro |
| `ref` callbacks on DOM nodes | Use `useRef` + island hydration |
| Context providers outside root | Simplified context model |
| Portals | Not applicable to SSR |
| Suspense (data fetching) | Async route handlers cover this |
| Error boundaries | Route-level error pages |
| `useLayoutEffect` | `useEffect` suffices for SSR |
| `useImperativeHandle` | Refs are data-only |

#### Type System
| Feature | Rationale |
|---------|-----------|
| Complex conditional types | Erased; use runtime checks |
| Mapped types | Erased; code generation handles it |
| Template literal types | Limited value |
| `infer` | Too complex for subset |
| Recursive type aliases | Blocked to prevent infinite types |

---

## 2. Architecture & Transpilation Strategy

### 2.1 High-Level Pipeline

```
TS/TSX Source
    │
    ▼
┌─────────────────┐
│  Parser         │  Recursive descent, zero dependencies
│  (TSX → HIR)    │  ~3,500 LOC, handles 95%+ of Fresh code
└─────────────────┘
    │
    ▼
┌─────────────────┐
│  Analyzer       │  Type inference, island/route detection,
│  (Semantic)     │  hook validation, module classification
└─────────────────┘
    │
    ▼
┌─────────────────┐
│  Code Generator │  HIR → Rust source via in-memory codegen
│  (HIR → Rust)   │  No temp files; direct string building
└─────────────────┘
    │
    ▼
┌─────────────────┐
│  Cargo Build    │  Release-optimized native binary
│  (Native Rust)  │  Axum + Tower HTTP server
└─────────────────┘
```

### 2.2 Parser (TSX → HIR)

**Design decision:** Custom recursive descent parser instead of SWC/Babel.

- **Zero dependencies:** No 200MB parser crate. The entire parser is ~3,500 LOC.
- **TSX-native:** JSX is preserved as first-class `JSXExpr` nodes, not desugared to `React.createElement`.
- **Normalized output:** `var` → `let`, destructuring expanded, defaults inlined.

**Parsing pipeline:**
1. Tokenize (identifiers, literals, operators, JSX tokens)
2. Parse module → `Vec<ModuleItem>`
3. Extract imports/exports, function declarations, type definitions
4. Build `Module` with typed AST nodes

### 2.3 HIR (High-Level IR)

```rust
pub struct Module {
    pub source: String,
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}

pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),      // Erased at codegen except for struct gen
    Class(ClassDecl),    // Detected → E010 error
}
```

Key properties:
- **Typed but erasable:** Every expression carries a `Type`, but codegen ignores it except for struct generation.
- **JSX-native:** `JSXExpr` is a first-class node with `opening`, `children`, and `attrs`.

### 2.4 Semantic Analyzer

Responsibilities:
1. **Module classification:** Categorizes files as `Route`, `Island`, `Component`, `Middleware`, `Layout`, or `App`.
2. **Hook rule checker:** Validates hook call order (no hooks in loops/conditionals).
3. **Type inference:** Infers prop types for component generation.
4. **Route extraction:** Parses file paths into URL patterns (`[id]` → `:id`, `[...slug]` → `*slug`).

### 2.5 Rust Code Generation

#### TS/TSX → Rust Transform Rules

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

#### JSX → `html!` Macro

JSX is transformed into the `html!` procedural macro:

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
</div>
)
```

The `html!` macro expands to `VNode::element()` builder calls at compile time.

#### Islands Codegen

Islands are generated with:
1. Props struct (`#[derive(Serialize, Deserialize)]`)
2. Component function (`#[component]`)
3. Island registration for SSR (`register_island`)
4. Client JS bundle generation (vanilla JS signals + hydration)

### 2.6 Runtime Architecture

#### Production Server Stack
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

#### Signal System (Fine-Grained Reactivity)

Leptos-inspired signals with automatic dependency tracking:

```rust
let count = Signal::new(0);
let doubled = Computed::new({
    let count = count.clone();
    move || count.get() * 2
});

Effect::new(move || {
    println!("Count: {}", count.get());
});

count.set(5); // Effect re-runs automatically
```

- **Thread-local effect tracking:** `push_effect`/`pop_effect` around signal reads.
- **Batching:** `batch(|| { ... })` defers effect execution.
- **No VDOM diffing for islands:** Signals update DOM directly via fine-grained subscriptions.

#### Islands Architecture & Partial Hydration

```
Server SSR:
  1. Render island component → VNode
  2. to_html() wraps with hydration markers:
     <div data-island="Counter" data-props="{...}">
       ...rendered HTML...
     </div>
  3. Inject island manifest + client runtime script

Client hydration:
  1. Browser loads vanilla JS runtime (~4KB gzipped)
  2. Discover islands: querySelectorAll('[data-island]')
  3. Bootstrap by strategy:
     - load: immediate
     - visible: IntersectionObserver
     - interaction: event listener
     - idle: requestIdleCallback
  4. Hydrate: parse props, render island, attach signals
```

### 2.7 Development Mode: HIR Interpreter

**Philosophy:** Zero compilation. Pure runtime execution with <100ms hot reload.

```
File change (notify crate)
    │
    ▼
Invalidate HIR cache entry
    │
    ▼
Re-parse changed file → HIR
    │
    ▼
Broadcast SSE event to browser
    │
    ▼
Browser reloads page (full page refresh)
```

Why full page refresh instead of HMR?
- **Correctness:** Interpreter state is fully reset; no stale closures or hooks.
- **Simplicity:** No module hot-swapping or React Fast Refresh equivalent.
- **Speed:** <50ms end-to-end means full refresh is acceptable.

**Interpreter capabilities:**
- Full route execution (handlers + component rendering)
- Middleware pipeline execution
- Island rendering with hydration markers
- Layout nesting and composition
- Error page fallback (404/500)

### 2.8 Production Mode: Native Compilation

**Build pipeline:**
1. Parse all TS/TSX → HIR
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

---

## 3. Roadmap: MVP → Full Fresh Coverage

### Phase 0: Foundation ✅ COMPLETE
- Parser, HIR, Analyzer, Codegen
- Signal system, Hooks, VDOM
- Islands architecture, File-based routing
- Dev server, Build command, Init command
- Example project (`examples/my-blog`)

### Phase 1: v0.6.0 — Feature Complete
| Feature | Priority | Status |
|---------|----------|--------|
| Source maps | P2 | ❌ Not started |
| Plugin system | P2 | ❌ Not started |
| `_app.tsx` root wrapper | P1 | ✅ Complete |
| Route groups `(group)/` | P2 | ✅ Complete |
| `useErrorBoundary` hook | P1 | ✅ Complete |
| CSS modules / Tailwind integration | P2 | ❌ Not started |
| Form handling (`ctx.formData`) | P1 | ❌ Not started |
| Database adapter layer | P2 | ❌ Not started |
| Static site generation (SSG) | P1 | ❌ Not started |
| Multi-language i18n | P3 | ❌ Not started |
| Parser migration to formal grammar | P1 | 🔄 Planned |

### Phase 2: v0.7.0 — Production Hardening
- Comprehensive error reporting with suggestions
- Binary size optimization (LTO, strip, panic=abort)
- Security audit (XSS prevention, header handling)
- Load testing and performance regression suite

### Phase 3: v0.8.0 — Developer Experience
- Fine-grained HMR (CSS + island props)
- VS Code extension / LSP for `.tsx` files
- Debugging support (source maps → Rust)
- Integration testing framework

### Phase 4: v0.9.0 — Ecosystem Integration
- Deno Deploy / Vercel / Netlify adapters
- Database ORM integration (Drizzle-style)
- NPM package compatibility layer
- Edge runtime support (Cloudflare Workers)

### Phase 5: v1.0.0 — Stable
- 100% Fresh API compatibility for supported subset
- Production deployments at scale
- Stable ABI for plugins
- Comprehensive documentation + tutorials

---

## 4. Performance Targets & Trade-offs

### 4.1 Production Binary

| Metric | Target | v0.5 Status | Methodology |
|--------|--------|-------------|-------------|
| **Binary size** | < 2 MB | ~1.8 MB | `cargo build --release` + strip |
| **Cold start** | < 10 ms | ~3 ms | `time ./target/release/my-blog` |
| **Memory baseline** | < 5 MB RSS | ~3.5 MB | `ps -o rss` at idle |
| **Request latency (p50)** | < 1 ms | ~0.3 ms | `wrk -t1 -c1 -d10s` |
| **Request latency (p99)** | < 5 ms | ~2 ms | `wrk -t4 -c100 -d30s` |
| **Throughput** | > 100K RPS | ~120K RPS | `wrk` on localhost |

### 4.2 Development Mode

| Metric | Target | v0.5 Status | Methodology |
|--------|--------|-------------|-------------|
| **Hot reload** | < 100 ms | ~40 ms | File change → browser refresh |
| **Cold start (dev)** | < 500 ms | ~300 ms | `runts dev` startup |
| **Memory (dev)** | < 50 MB | ~35 MB | `ps -o rss` |

### 4.3 Client Runtime (Browser)

| Metric | Target | v0.5 Status | Methodology |
|--------|--------|-------------|-------------|
| **Client runtime size** | < 5 KB gzipped | ~4.2 KB | `gzip -c runtime.ts | wc -c` |
| **Hydration start delay** | < 16 ms | < 10 ms | `requestAnimationFrame` to first hydrate |
| **Visible island TTI** | < 50 ms | < 30 ms | IntersectionObserver → interactive |

### 4.4 Trade-offs

| Decision | Trade-off | Rationale |
|----------|-----------|-----------|
| Custom parser vs SWC | ~2 weeks dev time vs 200MB dep | Binary size and build time critical |
| HIR interpreter in dev | No native speed | Sub-100ms reload acceptable; correctness paramount |
| Full compilation in prod | ~30s build time | Native performance worth the wait |
| VDOM for SSR, signals for islands | Two rendering paths | SSR benefits from VDOM tree; islands need fine-grained updates |
| No JS runtime | Limited dynamic evaluation | Fresh apps don't need `eval` |
| Full page refresh in dev | Slower than HMR | Prevents entire class of state bugs |
| `f64` for all numbers | No integer overflow protection | Matches JS semantics exactly |
| Thread-local hooks | No multi-threaded rendering | Preact hooks are single-threaded by design |

---

## 5. Verification

### Test Coverage
- **Unit tests:** 106 passing (parser, codegen, hooks, signals, routing, error boundaries)
- **Integration tests:** End-to-end dev server request handling
- **Example project:** `examples/my-blog` — 4 routes, 3 islands, 1 component, middleware

### Build Verification
```bash
# Dev mode
cargo run -- dev

# Production build
cargo run -- build --release

# Tests
cargo test

# Example build
cd examples/my-blog && cargo build --release
```

---

*runts is designed to prove that framework-level Fresh/Preact compatibility can be achieved with zero external JS runtimes, compiling to efficient native binaries through a well-defined subset of TypeScript + TSX.*
