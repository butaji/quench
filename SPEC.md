# runts - Specification v0.3.0

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
| `useEffect` | ✅ | SSR-safe |
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
| Layouts | ✅ | `_layout.tsx` |
| Middleware | ✅ | `_middleware.ts` |
| Islands | ✅ | `islands/**/*.tsx` |
| `PageProps` | ✅ | Typed route params |
| `HandlerContext` | ✅ | Full context |
| `Handler` export | ✅ | Route handlers |
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

## Part IV: Development vs Production Modes

### 4.1 Development Mode

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

### 4.2 Production Mode

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

## Part V: Roadmap

### 5.1 Current Status (v0.3.0)

| Feature | Status | Notes |
|---------|--------|-------|
| TS/TSX Parser | ✅ | Custom recursive descent |
| HIR | ✅ | Complete IR |
| Rust Codegen | ✅ | Most patterns |
| Hooks | ✅ | useState, useEffect, etc. |
| Signals | ✅ | Fine-grained reactivity |
| Dev Server | ✅ | HIR interpreter |
| Islands | ⚠️ | Partial - needs client JS |
| Layouts | ⚠️ | Partial - needs composer |
| Middleware | ⚠️ | Partial - needs pipeline |

### 5.2 MVP Completion (v0.4.0)

| Feature | Priority | Description |
|---------|----------|-------------|
| Client Island JS | P0 | Generate minimal hydration bundles |
| Layout Composition | P0 | Proper `_layout.tsx` rendering |
| Middleware Pipeline | P1 | Connect to Axum tower |
| Error Pages | P1 | `_404.tsx`, `_500.tsx` |
| State Sharing | P2 | `ctx.state` between middleware/handlers |

### 5.3 v1.0 Roadmap

| Feature | Priority | Description |
|---------|----------|-------------|
| Full SSR Streaming | P1 | Incremental HTML output |
| Asset Pipeline | P2 | CSS/JS bundling |
| Image Optimization | P3 | Built-in handling |
| Edge Deployment | P3 | WASM target (optional) |

---

## Part VI: Performance Targets

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

## Part VII: Type Mappings

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

*Document Version: 0.3.0*  
*Last Updated: 2026-05-26*
