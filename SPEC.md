# runts - Specification v0.4.0

## Executive Summary

**runts** is a Fresh/Preact-compatible TypeScript framework that compiles to native Rust binaries with zero external JS runtime dependencies. It provides framework-level compatibility with Fresh's islands architecture while achieving native binary performance.

### Key Properties

| Property | Value |
|----------|-------|
| **Approach** | TS/TSX → HIR → In-memory Rust codegen → Native binary |
| **Runtime** | Pure Rust (Axum + custom Preact-compatible runtime) |
| **Dev Mode** | HIR interpreter (no Rust recompilation, <100ms hot-reload) |
| **Production** | Full static compilation, single binary |
| **Binary Size Target** | <2MB (with full HTTP server, SSR, routing) |
| **Cold Start** | <10ms (embedded HTTP server) |
| **Memory Baseline** | <5MB RSS |

---

## Part I: Supported TypeScript/TSX Subset

### 1.1 Design Principles

1. **95%+ Coverage First**: Support common patterns, defer edge cases
2. **Predictable Transpilation**: No runtime reflection, pure codegen
3. **Type Safety**: Emit typed Rust, leverage Rust's compiler
4. **Minimal Runtime**: Runtime helpers kept to strict minimum

### 1.2 Supported Features

#### ✅ Language Features

| Feature | Syntax | Status |
|---------|--------|--------|
| Variables | `const`, `let`, `var` | ✅ Full |
| Functions | `function`, arrow functions, generators | ✅ Full |
| Types | Primitives, unions, intersections, generics | ✅ Full |
| Interfaces | `interface` | ✅ Full |
| Type aliases | `type X = ...` | ✅ Full |
| Enums | `enum` | ✅ Full |
| Destructuring | Object & array patterns | ✅ Full |
| Spread | `...expr` | ✅ Full |
| Template literals | `` `hello ${x}` `` | ✅ Full |
| Optional chaining | `a?.b?.c` | ✅ Full |
| Nullish coalescing | `a ?? b` | ✅ Full |
| Type assertions | `expr as Type` | ✅ Stripped |
| Classes | `class` | ❌ Not supported |
| `with` statement | `with (obj) { }` | ❌ Excluded |
| `eval` | `eval(code)` | ❌ Excluded |
| Dynamic imports | `import()` | ❌ Static only |

#### ✅ JSX/TSX Support

| Feature | Example | Status |
|---------|---------|--------|
| Elements | `<div>...</div>` | ✅ HTML + SVG |
| Components | `<Counter />` | ✅ PascalCase |
| Fragments | `<>...</>` | ✅ |
| Props | `prop={value}` | ✅ |
| Events | `onClick={handler}` | ✅ |
| Spread | `<div {...props} />` | ✅ |
| Children | `<Parent>{child}</Parent>` | ✅ |
| Conditional | `{condition && <X />}` | ✅ |
| Loops | `{items.map(x => <X />)}` | ✅ |

#### ✅ Preact Hooks

| Hook | Status | Notes |
|------|--------|-------|
| `useState` | ✅ | Full |
| `useEffect` | ⚠️ | SSR-safe (client only) |
| `useRef` | ✅ | Via Ref<T> |
| `useMemo` | ✅ | Basic |
| `useCallback` | ✅ | Function memo |
| `useReducer` | ✅ | Full |
| `useContext` | ✅ | Context pattern |
| `useId` | ✅ | Unique IDs |
| `useSignal` | ✅ | Preact Signals |
| `useComputed` | ✅ | Derived signals |
| `useSignalEffect` | ✅ | Signal effects |

#### ✅ Fresh-Specific

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ | `routes/**/*.tsx` |
| Route patterns | ✅ | Static, param, catch-all |
| Layouts | ⚠️ | Partial - needs composition |
| Middleware | ⚠️ | Partial - pipeline needs work |
| Islands | ⚠️ | SSR placeholder - client needs work |
| `PageProps` | ✅ | Typed route params |
| `HandlerContext` | ⚠️ | Partial |
| `Handler` export | ⚠️ | GET handler only |
| `Default` export | ✅ | Page components |
| `State` | ⚠️ | Partial |

### 1.3 Explicitly Excluded Features

#### ❌ Not Supported

| Feature | Reason | Workaround |
|---------|--------|------------|
| `with` statement | Not in Rust | Destructure |
| `eval` | Security/Codegen | N/A |
| Dynamic `import()` | Requires bundler | Static imports |
| `require()` | CommonJS | ES modules only |
| Class components | Runtime complexity | Functional only |
| Decorators | Stage-2 proposal | Component attrs |
| JSDoc types | Redundant | TypeScript types |
| Conditional types | Complex inference | Explicit unions |
| Recursive types | Infinite codegen | Explicit base |

---

## Part II: Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        runts Architecture                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  User Code (TS/TSX)                                                 │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ routes/, islands/, components/, middleware/                  │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    Transpiler Pipeline                       │   │
│  │  ┌───────────┐  ┌────────────┐  ┌───────────────────────┐  │   │
│  │  │  Parser   │─▶│  Analyzer  │─▶│   Code Generator      │  │   │
│  │  │  (HIR)   │  │ (Semantic) │  │   (Rust source)       │  │   │
│  │  └───────────┘  └────────────┘  └───────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│              ┌───────────────┴───────────────┐                      │
│              ▼                               ▼                      │
│  ┌─────────────────────┐         ┌─────────────────────┐         │
│  │   Development Mode   │         │   Production Mode    │         │
│  │                      │         │                      │         │
│  │  HIR → Interpreter    │         │  Rust codegen →      │         │
│  │  (direct execution)  │         │  cargo build         │         │
│  │                      │         │  (static binary)    │         │
│  │  File watcher        │         │                      │         │
│  │  Instant HMR         │         │  Axum routes         │         │
│  │  (<100ms)            │         │  Islands hydration   │         │
│  └─────────────────────┘         └─────────────────────┘         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Runtime Architecture

#### Server Runtime Stack

```
┌─────────────────────────────────────────────────────────────────┐
│                     Server Runtime Stack                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                  Axum HTTP Server                         │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │   │
│  │  │  Router     │  │  Middleware │  │  Static Files   │   │   │
│  │  │  (routes)   │  │  (tower)    │  │  (tower-http)  │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘   │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 SSR Renderer                               │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │   │
│  │  │  Component  │  │  Islands    │  │  Layouts        │   │   │
│  │  │  Renderer   │  │  Placeholder │  │  Composer        │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘   │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                 VDOM / Template Engine                     │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │   │
│  │  │  VNode      │  │  Hydrate    │  │  HTML Writer     │   │   │
│  │  │  Builder    │  │  Markers    │  │  (escaped)      │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘   │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.3 Islands Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Islands Architecture                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  SSR Output:                                                     │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ <html>                                                    │   │
│  │   <head>...</head>                                        │   │
│  │   <body>                                                  │   │
│  │     <nav>...</nav>            ← Static HTML               │   │
│  │     <main>                      ← Static HTML             │   │
│  │       <div data-island="Counter" data-id="abc123"        │   │
│  │         data-props='{"initial":0,"step":1}'>            │   │
│  │         <p>Count: 5</p>         ← SSR placeholder        │   │
│  │       </div>                                              │   │
│  │     </main>                                                │   │
│  │     <script>window.__RUNTS_ISLANDS__ = [...];</script>   │   │
│  │   </body>                                                 │   │
│  │ </html>                                                   │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Client Hydration:                                              │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  1. Parse islands manifest                                 │   │
│  │  2. Register island components                            │   │
│  │  3. For each island:                                      │   │
│  │    a. Match SSR HTML by data-id                         │   │
│  │    b. Attach event listeners                            │   │
│  │    c. Restore component state from data props           │   │
│  │  4. Mark as hydrated                                     │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Hydration Modes:                                                │
│  - Eager: Immediate on page load                               │
│  - Visible: IntersectionObserver (default)                       │
│  - Idle: requestIdleCallback                                   │
│  - Manual: On explicit trigger                                  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Part III: File Structure

```
runts-project/
├── routes/                      # File-based routing
│   ├── _middleware.ts          # Global middleware
│   ├── _layout.tsx             # Root layout
│   ├── _app.tsx               # App wrapper
│   ├── _404.tsx               # 404 page
│   ├── index.tsx               # GET /
│   ├── about.tsx               # GET /about
│   └── blog/
│       ├── _layout.tsx         # Blog section layout
│       ├── index.tsx           # GET /blog
│       └── [slug].tsx          # GET /blog/:slug
│
├── islands/                     # Interactive components
│   ├── Counter.tsx             # Hydrated counter
│   ├── TodoList.tsx            # Interactive todo
│   └── SearchBox.tsx           # Live search
│
├── components/                  # Shared (static) components
│   ├── Header.tsx
│   ├── Footer.tsx
│   └── Button.tsx
│
├── lib/                        # Shared utilities
│   ├── db.ts
│   └── auth.ts
│
├── static/                     # Static assets
│   ├── favicon.ico
│   └── styles.css
│
├── runts.config.json           # Runts configuration
├── Cargo.toml                  # Rust dependencies
└── src/
    ├── main.rs                 # Entry point (generated)
    ├── lib.rs                  # Library (generated)
    └── gen/                    # Generated code
        ├── routes.rs
        ├── islands.rs
        └── components.rs
```

---

## Part IV: Implementation Status (v0.4.0)

### 4.1 Completed Components

| Component | Status | Notes |
|-----------|--------|-------|
| TS/TSX Parser | ✅ | Custom recursive descent |
| HIR (High-Level IR) | ✅ | Complete IR with all node types |
| Semantic Analyzer | ✅ | Detects islands, routes, hooks |
| Rust Codegen | ✅ | Most patterns covered |
| Hooks Runtime | ✅ | useState, useEffect, useRef, etc. |
| Signals Runtime | ✅ | Signal, Computed, batch |
| Islands Runtime | ✅ | IslandRenderer, Registry, Hydration |
| Dev Server | ✅ | HIR interpreter with hot reload |
| CLI | ✅ | init, dev, build, transpile, add |
| Client Runtime | ✅ | JavaScript hydration framework |

### 4.2 Incomplete Components

| Component | Priority | Description |
|-----------|----------|-------------|
| Route Handlers | P0 | `export const handler = { GET, POST, ... }` |
| Layout Composition | P0 | `_layout.tsx` rendering pipeline |
| Middleware Pipeline | P0 | Connect to Axum tower |
| Page Data | P0 | `ctx.render({ data })` in handlers |
| Client Island JS | P1 | Generate minimal hydration bundles |
| Error Pages | P1 | `_404.tsx`, `_500.tsx` |
| State Sharing | P2 | `ctx.state` between middleware/handlers |

---

## Part V: Development vs Production Modes

### 5.1 Development Mode

**Start**: `runts dev`

**Flow**:
1. Scan project structure
2. Pre-load all TS/TSX modules to HIR
3. Build route table from file paths
4. Start file watcher (notify)
5. Start Axum server with HIR interpreter
6. Serve with instant hot-reload

**Request Flow**:
```
Request → Route Match → Load HIR (cached) → Execute Handler
    → Call Hooks → Render Component → Compose Layouts
    → Inject Island Markers → HTML Response
```

**Hot Reload**:
- File change → Invalidate cache → Re-parse → Broadcast SSE → Browser refresh
- **Target**: <100ms from file save to visible update

### 5.2 Production Mode

**Build**: `runts build`

**Phase 1: Transpile**
```
for each TS/TSX file:
    1. Parse → HIR
    2. Analyze → validated HIR
    3. Generate → Rust source
    4. Write to src/gen/
```

**Phase 2: Compile**
```
cargo build --release
    ├── LTO enabled
    ├── single codegen unit
    ├── panic = abort
    └── static linking
```

---

## Part VI: Roadmap

### 6.1 v0.4.0 - MVP Completion

| Feature | Priority | Description |
|---------|----------|-------------|
| Route Handler Exports | P0 | `export const handler = { GET, POST }` |
| Layout Composition | P0 | Proper `_layout.tsx` rendering |
| Middleware Pipeline | P0 | Connect to Axum tower middleware |
| Page Data | P0 | Handler `ctx.render({ data })` |
| Client Island JS | P1 | Generate minimal hydration bundles |
| Error Pages | P1 | `_404.tsx`, `_500.tsx` |
| State Sharing | P2 | `ctx.state` between middleware/handlers |

### 6.2 v0.5.0 - Feature Complete

| Feature | Priority | Description |
|---------|----------|-------------|
| Full SSR Streaming | P1 | Incremental HTML output |
| Asset Pipeline | P2 | CSS/JS bundling |
| Image Optimization | P3 | Built-in handling |
| Edge Deployment | P3 | WASM target (optional) |

### 6.3 v1.0 Roadmap

| Feature | Priority | Description |
|---------|----------|-------------|
| Production HMR | P1 | Fine-grained module updates |
| Edge Functions | P2 | Serverless deployment |
| API Routes | P2 | Full REST API support |
| Database Integration | P2 | Prisma/Drizzle ORM |

---

## Part VII: Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| **Binary size** | <2MB | ~2.6MB |
| **Memory (baseline)** | <5MB RSS | N/A |
| **Cold start** | <10ms | ~50ms |
| **Hot request** | <1ms | N/A |
| **SSR throughput** | >50k req/s | N/A |
| **Dev HMR** | <100ms | <50ms |

### Trade-off Decisions

| Decision | Chosen | Rationale |
|----------|--------|-----------|
| Parser | Custom | Control subset, no dep |
| Runtime | Rust-only | No JS engine, max perf |
| Reactivity | Signals | Fine-grained, efficient |
| Hydration | Islands | Minimal JS, max perf |
| Codegen | In-memory | Fast builds, no temp files |
| Async runtime | Tokio | Battle-tested, async/await |
| HTTP server | Axum | Type-safe, tower integration |

---

## Part VIII: Type Mappings

| TypeScript | Rust |
|------------|------|
| `string` | `String` |
| `number` | `f64` |
| `boolean` | `bool` |
| `null` | `Option<T>::None` |
| `undefined` | `()` |
| `T[]` | `Vec<T>` |
| `{ a: T }` | `{ a: T }` (struct) |
| `T \| null` | `Option<T>` |
| `Record<K,V>` | `HashMap<K, V>` |
| `Promise<T>` | `impl Future<Output = T>` |
| `PageProps<P>` | `PageProps<P>` |
| `HandlerContext` | `HandlerContext` |

---

## Appendix A: Error Codes

| Code | Meaning | Resolution |
|------|---------|------------|
| E001 | Parse error | Check TS/TSX syntax |
| E002 | Type error | Fix type annotations |
| E003 | Unsupported feature | Check exclusion list |
| E004 | Island in route | Move to islands/ |
| E005 | Missing handler | Export handler |
| E006 | Invalid route pattern | Fix route file name |
| E007 | Import error | Check import paths |
| E008 | Build error | Check generated Rust |

---

## Appendix B: Configuration

```json
{
  "server": {
    "port": 8000,
    "host": "127.0.0.1"
  },
  "islands": {
    "hydration": "visible",
    "serializer": "json"
  },
  "dev": {
    "port": 8000,
    "open": true,
    "hmr": true
  },
  "build": {
    "optimization": {
      "lto": true,
      "opt_level": "z"
    }
  }
}
```

---

## Appendix C: Supported Hooks API

### useState

```typescript
const [state, setState] = useState(initialValue);
// or with function initializer
const [state, setState] = useState(() => computeInitial());
```

### useEffect

```typescript
useEffect(() => {
  // effect code
  return () => {
    // cleanup
  };
}, [dependencies]);
```

### useRef

```typescript
const ref = useRef<T>(initialValue);
// Access via ref.current
```

### useMemo

```typescript
const value = useMemo(() => expensiveComputation(a, b), [a, b]);
```

### useCallback

```typescript
const handler = useCallback((event: Event) => {
  doSomething(event);
}, [dependencies]);
```

### useReducer

```typescript
const [state, dispatch] = useReducer((state, action) => {
  switch (action.type) {
    case 'increment': return { count: state.count + 1 };
    default: return state;
  }
}, { count: 0 });
```

### useContext

```typescript
const value = useContext(MyContext);
```

---

## Appendix D: Route File Conventions

### Static Routes
```
routes/index.tsx         → /
routes/about.tsx         → /about
routes/blog/index.tsx    → /blog
```

### Dynamic Routes
```
routes/blog/[slug].tsx   → /blog/:slug
routes/[year]/[month].tsx → /:year/:month
```

### Catch-all Routes
```
routes/[...path].tsx     → /*path
routes/api/[...rest].tsx → /api/*rest
```

### Layouts
```
routes/_layout.tsx       → Wraps all routes
routes/blog/_layout.tsx   → Wraps /blog/*
```

### Special Files
```
routes/_middleware.ts    → Global middleware
routes/_app.tsx          → App wrapper
routes/_404.tsx          → 404 page
```

---

## Appendix E: Island Conventions

```typescript
// islands/Counter.tsx
// Components in islands/ are automatically hydrated on the client

import { useState } from "preact/hooks";

interface Props {
  initial?: number;
  step?: number;
}

export default function Counter({ initial = 0, step = 1 }: Props) {
  const [count, setCount] = useState(initial);
  
  return (
    <div>
      <p>Count: {count}</p>
      <button onClick={() => setCount(c => c + step)}>+</button>
    </div>
  );
}
```

Islands are rendered server-side for SSR and hydrated on the client for interactivity.

---

*Document Version: 0.4.0*  
*Last Updated: 2026-05-26*
