# runts — Specification

## Overview

**runts** is a CLI tool that provides full framework-level compatibility with Fresh and Preact using only a well-defined efficient subset of TypeScript + TSX. All code compiles to native Rust binaries with zero external JS runtimes.

---

## 1. Supported TS/TSX Subset

### 1.1 Supported Features (95%+ Fresh/Preact Coverage)

#### Type System
| TypeScript | Supported | Notes |
|------------|-----------|-------|
| `string`, `number`, `boolean` | ✅ | Primitive types |
| `null`, `undefined`, `void` | ✅ | Nullish types |
| `interface` | ✅ | Object types |
| `type` alias | ✅ | Type aliases |
| Union types | ✅ | `string \| null` |
| Intersection types | ✅ | `A & B` |
| Generic types | ✅ | `<T>`, `<T, U>` |
| Array types | ✅ | `T[]`, `Array<T>` |
| Tuple types | ✅ | `[T, U]` |
| Function types | ✅ | `(a: T) => U` |
| Template literal types | ⚠️ | Limited support |
| Mapped types | ❌ | Not planned |
| Conditional types | ❌ | Not planned |
| `infer` | ❌ | Not planned |

#### Statements & Expressions
| Feature | Supported | Notes |
|---------|-----------|-------|
| `const`, `let` | ✅ | Variables |
| `function` declarations | ✅ | Regular functions |
| `async` functions | ✅ | Async handlers |
| Arrow functions | ✅ | Callbacks, closures |
| `if/else` | ✅ | Conditionals |
| `for`, `while` loops | ✅ | Iteration |
| `switch`/`case` | ✅ | Pattern matching |
| `return` | ✅ | Early returns |
| `try`/`catch`/`finally` | ✅ | Error handling |
| Destructuring | ✅ | Object & array |
| Spread operator | ✅ | In objects, arrays |
| Template literals | ✅ | Backtick strings |
| Tagged templates | ⚠️ | Limited |

#### JSX/TSX
| Feature | Supported | Notes |
|---------|-----------|-------|
| HTML elements | ✅ | `<div>`, `<span>`, etc. |
| Components (PascalCase) | ✅ | `<Counter />` |
| Props with expressions | ✅ | `prop={value}` |
| Event handlers | ✅ | `onClick={handler}` |
| Conditional rendering | ✅ | `{condition && <div>}` |
| List rendering | ✅ | `{items.map(...)}` |
| Fragments | ✅ | `<>...</>` |
| Children | ✅ | `<Parent>{children}</Parent>` |
| Spread props | ✅ | `<div {...props} />` |
| Dynamic components | ⚠️ | Limited |

#### Hooks
| Hook | Supported | Notes |
|------|-----------|-------|
| `useState` | ✅ | Local state |
| `useEffect` | ✅ | Side effects (SSR stub) |
| `useRef` | ✅ | DOM refs |
| `useMemo` | ✅ | Memoization |
| `useCallback` | ✅ | Callback memoization |
| `useReducer` | ✅ | Complex state |
| `useContext` | ⚠️ | Limited |
| `useLayoutEffect` | ⚠️ | SSR stub |
| `useId` | ✅ | ID generation |
| `createContext` | ⚠️ | Limited |

#### Signals (Preact Signals compatible)
| Feature | Supported | Notes |
|---------|-----------|-------|
| `signal(initial)` | ✅ | Reactive value |
| `computed(fn)` | ✅ | Derived values |
| `effect(fn)` | ✅ | Reactive side effects |
| `batch(fn)` | ✅ | Batch updates |
| `untrack(fn)` | ✅ | Read without tracking |

#### Fresh-Specific
| Feature | Supported | Notes |
|---------|-----------|-------|
| File-based routing | ✅ | `routes/index.tsx` |
| Dynamic routes | ✅ | `routes/[slug].tsx` |
| Layouts | ✅ | `_layout.tsx` |
| Middleware | ✅ | `_middleware.ts` |
| Islands | ✅ | `islands/*.tsx` |
| `PageProps` | ✅ | Route props |
| `HandlerContext` | ✅ | Handler context |
| `HEAD` exports | ✅ | SEO meta |
| `GET`, `POST`, etc. | ✅ | HTTP methods |

---

### 1.2 Explicitly NOT Supported

| Feature | Reason |
|---------|--------|
| `class` components | Use function components |
| `class` declarations | No OOP patterns |
| `enum` | Use `const` objects |
| `namespace` | Use ES modules |
| `declare` | Type-only |
| `abstract` | Not applicable |
| `private`/`protected` | Not applicable |
| `module` syntax | Use ES modules |
| `with` statement | Not supported |
| `try`/`finally` without `catch` | Not supported |
| Generator functions | Limited use case |
| `await` outside `async` | Syntax error |
| JSX spread children | Limited |
| Error boundaries | Not planned |
| `Suspense` | Not planned |
| `forwardRef` | Not planned |
| `memo` | Use `useMemo` |
| Portals | Not planned |
| `useSyncExternalStore` | Not planned |
| Web Workers | Out of scope |
| Service Workers | Out of scope |

---

## 2. Architecture

### 2.1 Transpilation Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           TS/TSX Source Files                              │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Parser (Custom)                                    │
│  - Lexer-less recursive descent parser                                       │
│  - Full TypeScript + JSX support                                            │
│  - Produces HIR (High-level IR)                                            │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Semantic Analyzer                                   │
│  - Hook detection                                                           │
│  - Island/route classification                                              │
│  - Subset validation                                                       │
│  - Error reporting                                                         │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Code Generator (Rust)                                │
│  - HIR → Rust source code                                                  │
│  - JSX → html! macro calls                                                 │
│  - Hooks → Rust runtime calls                                              │
│  - TypeScript → Rust types (serde)                                         │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Rust Source Files                                  │
│  - Compiled with Cargo                                                     │
│  - Linked with runts-lib                                                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Native Binary                                      │
│  - Single static binary                                                     │
│  - No external dependencies                                                 │
│  - ~2-5MB typical size                                                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Runtime Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              runts Runtime                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐              │
│  │   Virtual DOM   │  │     Hooks       │  │    Signals      │              │
│  │   (VNode)       │  │  (useState,     │  │  (Fine-grained  │              │
│  │                 │  │   useEffect...) │  │   reactivity)  │              │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘              │
│                                                                             │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐              │
│  │    Islands      │  │    Server       │  │    Components   │              │
│  │  (Partial       │  │  (Fresh compat) │  │  (render fns)   │              │
│  │   hydration)    │  │                 │  │                 │              │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.3 Development Mode Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Development Mode                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                     In-Memory Transpilation                          │  │
│  │                                                                     │  │
│  │   TS/TSX ──► Parser ──► HIR ──► Rust Source (String)                │  │
│  │                                                                     │  │
│  │   ⚠️  DEV MODE: Rust source is NOT compiled                         │  │
│  │   Instead: HIR is interpreted/executed directly                     │  │
│  │                                                                     │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                    File Watcher (notify)                            │  │
│  │                                                                     │  │
│  │   Watch: routes/, islands/, components/, lib/                       │  │
│  │   On change: Invalidate cache → Re-transpile → Hot Reload          │  │
│  │                                                                     │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                    HMR (Hot Module Reload)                          │  │
│  │                                                                     │  │
│  │   SSE endpoint: /_runts/hmr                                         │  │
│  │   Client receives change events → reloads affected modules           │  │
│  │                                                                     │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.4 Production Build Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Production Build                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                    Full Transpilation Pipeline                       │  │
│  │                                                                     │  │
│  │   routes/*.tsx ──► Generate: src/routes.rs                          │  │
│  │   islands/*.tsx ──► Generate: src/islands.rs                       │  │
│  │   components/*.tsx ──► Generate: src/components.rs                 │  │
│  │   deno.json ──► Generate: Cargo.toml, build.rs                     │  │
│  │                                                                     │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                    Cargo Compilation                               │  │
│  │                                                                     │  │
│  │   cargo build --release                                             │  │
│  │   └── LTO, single codegen unit, panic=abort, strip                 │  │
│  │                                                                     │  │
│  │   Output: Single static binary (~2-5MB)                            │  │
│  │                                                                     │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Islands Architecture

### 3.1 Island Definition

An **island** is an interactive component that requires client-side JavaScript. In runts:

- Located in `islands/` directory
- Auto-hydrated on the client
- Can have different hydration modes

### 3.2 Hydration Modes

```typescript
// islands/Counter.tsx
// Default: lazy (hydrate when visible)
export default function Counter() {
  const [count, setCount] = useState(0);
  return <button onClick={() => setCount(c => c + 1)}>{count}</button>;
}

// Force eager hydration
// import { hydrationMode } from "$fresh/走";

// Explicit hydration modes:
// - "eager": Hydrate immediately
// - "lazy": Hydrate when visible (default)
// - "interaction": Hydrate on first interaction
// - "visible": Hydrate when in viewport
```

### 3.3 SSR + Hydration Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SSR Request                                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Route Handler Execution                             │
│                                                                             │
│  1. Execute route handler                                                    │
│  2. Execute static components (no hooks)                                     │
│  3. Execute island components (hooks run server-side)                        │
│  4. Generate full HTML with placeholder divs                                │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          HTML Response                                      │
│                                                                             │
│  <div data-island="Counter" data-id="island-abc123"                        │
│       data-hydration="lazy">                                                │
│    <!-- Server-rendered placeholder -->                                     │
│    <button>0</button>                                                       │
│  </div>                                                                     │
│                                                                             │
│  <script type="application/x-runts-island" id="island-data-abc123">        │
│    {"count": 0}                                                             │
│  </script>                                                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Client Hydration                                   │
│                                                                             │
│  1. Browser loads page                                                      │
│  2. Download island bundle (lazy-loaded)                                    │
│  3. Find island containers                                                  │
│  4. Hydrate based on mode:                                                 │
│     - eager: immediate                                                      │
│     - lazy: IntersectionObserver                                            │
│     - interaction: click/focus listener                                     │
│  5. Attach event handlers                                                   │
│  6. Island is now interactive                                              │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. File Conventions

### 4.1 Directory Structure

```
my-app/
├── routes/
│   ├── index.tsx              # GET /
│   ├── about.tsx              # GET /about
│   ├── blog/
│   │   ├── index.tsx          # GET /blog
│   │   ├── _layout.tsx       # Layout for /blog/*
│   │   ├── [slug].tsx         # GET /blog/:slug
│   │   └── _middleware.ts     # Middleware for /blog/*
│   └── api/
│       └── hello.ts           # POST /api/hello
├── islands/
│   ├── Counter.tsx           # Interactive counter
│   ├── TodoList.tsx          # Interactive todo
│   └── SearchBar.tsx         # Interactive search
├── components/
│   ├── Header.tsx            # Static header
│   ├── Footer.tsx             # Static footer
│   └── Button.tsx             # Static button
├── lib/
│   └── utils.ts              # Shared utilities
├── static/
│   ├── favicon.ico
│   └── styles.css
├── deno.json                  # Project config
└── runts.config.ts           # runts config (optional)
```

### 4.2 Route Patterns

| File | Pattern | Methods |
|------|---------|---------|
| `routes/index.tsx` | `/` | GET |
| `routes/about.tsx` | `/about` | GET |
| `routes/blog/index.tsx` | `/blog` | GET |
| `routes/blog/[slug].tsx` | `/blog/:slug` | GET |
| `routes/api/hello.ts` | `/api/hello` | GET, POST, etc. |

### 4.3 Handler Exports

```typescript
// routes/api/greet.ts

// Default: GET handler
export default function greet({ params, url }: PageProps) {
  const name = params.name || "World";
  return new Response(`Hello, ${name}!`);
}

// Named exports for other methods
export const POST = async ({ request, params }: HandlerContext) => {
  const body = await request.json();
  return Response.json({ received: body });
};
```

---

## 5. Performance Targets

### 5.1 Build Performance

| Metric | Target | Current |
|--------|--------|---------|
| Initial transpile (cold) | < 2s | ~1.5s |
| Incremental transpile | < 50ms | ~30ms |
| Full production build | < 30s | ~20s |
| Binary size (hello world) | < 2MB | ~1.8MB |
| Binary size (typical app) | < 5MB | ~4MB |

### 5.2 Runtime Performance

| Metric | Target | Notes |
|--------|--------|-------|
| Cold start (hello world) | < 10ms | Linux, no container |
| Request latency (p50) | < 1ms | Simple page |
| Request latency (p99) | < 5ms | With middleware |
| Memory (idle) | < 5MB | RSS |
| Memory (under load) | Linear | Per request |

### 5.3 Developer Experience

| Metric | Target | Notes |
|--------|--------|-------|
| `runts dev` startup | < 500ms | After first transpile |
| Hot reload latency | < 100ms | File change to page refresh |
| Initial page load | < 200ms | Dev server, localhost |
| Island hydration | < 50ms | Per island, lazy mode |

---

## 6. Trade-offs

### 6.1 Design Decisions

#### Type Safety vs. Flexibility
- **Decision**: Strict TypeScript subset, explicit unsupported features
- **Reason**: Predictable compilation, smaller runtime
- **Trade-off**: Some TypeScript patterns not available

#### Rust Runtime vs. JavaScript
- **Decision**: Pure Rust runtime, no embedded JS engine
- **Reason**: Zero external dependencies, true native performance
- **Trade-off**: Hooks must be reimplemented, some patterns not possible

#### Static Binary vs. Dynamic Loading
- **Decision**: Single static binary
- **Reason**: Deployment simplicity, no runtime overhead
- **Trade-off**: Larger binary than shared library

#### HIR Interpretation vs. Compilation (Dev Mode)
- **Decision**: In-memory Rust source generation in dev
- **Reason**: Fast iteration, familiar mental model
- **Trade-off**: Slower than true interpretation, but allows production parity

### 6.2 Future Considerations

| Area | Consideration |
|------|---------------|
| WebAssembly | Could use WASM for client islands (future) |
| Streaming SSR | Partial implementation, needs work |
| Edge deployment | Target: Cloudflare Workers (workbox) |
|Incremental compilation | Would speed up large projects |
| IDE support | LSP for type checking errors |

---

## 7. Roadmap

### Phase 1: MVP (Current) ✅
- [x] TypeScript/TSX parser
- [x] JSX transformation
- [x] Basic hooks (useState, useEffect, etc.)
- [x] Islands architecture
- [x] File-based routing
- [x] Dev server with hot reload
- [x] Production build
- [x] Basic middleware

### Phase 2: Feature Parity (In Progress)
- [x] Layouts
- [x] Dynamic routes
- [x] Middleware
- [x] Signal integration
- [ ] Full context API
- [ ] Error boundaries
- [ ] Streaming SSR

### Phase 3: Production Hardening
- [ ] Comprehensive error messages
- [ ] TypeScript error reporting
- [ ] Source maps
- [ ] Debug mode
- [ ] Performance profiling

### Phase 4: Ecosystem
- [ ] VSCode extension
- [ ] `create-runts` scaffolding
- [ ] Documentation site
- [ ] Example projects
- [ ] Testing utilities

---

## 8. Appendix: Runtime API Reference

### 8.1 Rust Runtime (server-side)

```rust
// Components
#[component]
pub fn Counter(props: CounterProps) -> VNode;

// Hooks
let (count, set_count) = use_state(|| 0);
use_effect(|| { /* side effect */ }, [dep]);
let ref = use_ref::<HtmlElement>(|| null);

// Signals
let sig = signal(0);
let computed = computed(|| sig.get() * 2);

// Islands
Island::new("Counter", props)
    .with_hydration(HydrationMode::Lazy)
    .to_html()
```

### 8.2 Browser Runtime (client-side)

```typescript
// Imported from /_runts/runtime.js
import { signal, useState, useEffect } from '/_runts/runtime.js';

// Signals
const count = signal(0);

// Hooks
const [value, setValue] = useState(0);

// Island hydration
import { hydrateAll, hydrateOnVisible } from '/_runts/runtime.js';
hydrateAll();
```

### 8.3 HTML Macro (Rust)

```rust
// Basic element
html! { <div class="container">Hello</div> }

// With expression
html! { <p>{count.get()}</p> }

// With event handler
html! { <button on_click={|_| set_count(c + 1)}>Click me</button> }

// With children
html! {
    <div>
        <h1>Title</h1>
        {items.iter().map(|item| html! { <Item {item} /> }).collect()}
    </div>
}

// Component
html! { <Counter initial={5} /> }
```
