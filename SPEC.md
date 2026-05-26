# runts v0.5.0 - Technical Specification

## Overview

**runts** is a Fresh/Preact-compatible TypeScript framework that compiles to native Rust binaries with zero external JS runtime dependencies. It provides framework-level compatibility with Fresh's islands architecture while achieving native binary performance.

### Key Properties

| Property | Value |
|----------|-------|
| **Approach** | TS/TSX → HIR → In-memory Rust codegen → Native binary |
| **Runtime** | Pure Rust (Axum + custom Preact-compatible runtime) |
| **Dev Mode** | HIR interpreter (no Rust recompilation, <50ms hot-reload) |
| **Production** | Full static compilation, single binary |
| **Binary Size Target** | <2MB (with full HTTP server, SSR, routing) |
| **Cold Start** | <5ms (embedded HTTP server) |

---

## Part I: Supported TypeScript/TSX Subset

### 1.1 Design Principles

1. **95%+ Coverage First**: Support common patterns, defer edge cases
2. **Predictable Transpilation**: No runtime reflection, pure codegen
3. **Type Safety**: Emit typed Rust, leverage Rust's compiler
4. **Minimal Runtime**: Runtime helpers kept to strict minimum
5. **Fresh Compatibility**: Zero or minimal changes to existing Fresh code

### 1.2 Supported Features

#### ✅ Language Features

| Feature | Syntax | Status | Notes |
|---------|--------|--------|-------|
| Variables | `const`, `let`, `var` | ✅ Full | All three keywords |
| Functions | `function`, arrow functions | ✅ Full | Generators deferred |
| Types | Primitives, unions, intersections | ✅ Full | Limited generics |
| Interfaces | `interface` | ✅ Full | Extends only |
| Type aliases | `type X = ...` | ✅ Full | Conditional types excluded |
| Enums | `enum` | ✅ Full | String enums preferred |
| Destructuring | Object & array patterns | ✅ Full | Nested + rest patterns |
| Spread | `...expr` | ✅ Full | In objects/arrays/JSX |
| Template literals | `` `hello ${x}` `` | ✅ Full | Nested expressions |
| Optional chaining | `a?.b?.c` | ✅ Full | Computed + calls |
| Nullish coalescing | `a ?? b` | ✅ Full | Nested |
| Type assertions | `expr as Type` | ✅ Stripped | Runtime erasure |
| Classes | `class` | ❌ Not supported | Use functions |
| `with` statement | `with (obj) { }` | ❌ Excluded | Not in Rust |
| `eval` | `eval(code)` | ❌ Excluded | Security |
| Dynamic imports | `import()` | ❌ Static only | Use static imports |

#### ✅ JSX/TSX Support

| Feature | Example | Status | Notes |
|---------|---------|--------|-------|
| HTML Elements | `<div>...</div>` | ✅ Full | All HTML5 + SVG |
| Components | `<Counter />` | ✅ Full | PascalCase |
| Fragments | `<>...</>` | ✅ Full | `<Fragment>` too |
| Props | `prop={value}` | ✅ Full | Spread attrs |
| Events | `onClick={handler}` | ✅ Full | All DOM events |
| Spread | `<div {...props} />` | ✅ Full | |
| Children | `<Parent>{child}</Parent>` | ✅ Full | |
| Conditional | `{condition && <X />}` | ✅ Full | |
| Loops | `{items.map(x => <X />)}` | ✅ Full | |
| Dynamic tags | `<{tagName} />` | ⚠️ Deferred | v0.6 |
| Refs | `ref={refObj}` | ⚠️ Deferred | |

#### ✅ Preact Hooks

| Hook | Status | Notes |
|------|--------|-------|
| `useState` | ✅ Full | With type inference |
| `useEffect` | ✅ Full | Cleanup supported |
| `useRef` | ✅ Full | `useRef<T>(null)` |
| `useMemo` | ✅ Full | Basic memoization |
| `useCallback` | ✅ Full | Function memo |
| `useReducer` | ✅ Full | Full reducer pattern |
| `useContext` | ✅ Full | Context pattern |
| `useId` | ✅ Full | Stable IDs |
| `useSignal` | ✅ Full | Preact Signals |
| `useComputed` | ✅ Full | Derived signals |
| `useSignalEffect` | ✅ Full | Signal effects |

#### ✅ Fresh-Specific

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ Full | All patterns |
| Route patterns | ✅ Full | Static, param, catch-all |
| Layouts | ✅ Full | `_layout.tsx` |
| Middleware | ✅ Full | `_middleware.ts` |
| Islands | ✅ Full | `islands/` directory |
| `PageProps` | ✅ Full | Typed params |
| `HandlerContext` | ✅ Full | Full context |
| `Handler` export | ✅ Full | All HTTP methods |
| `Default` export | ✅ Full | Page components |
| `State` | ✅ Full | Middleware state |

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
| Namespace merging | Complexity | Use modules |

#### ⚠️ Deferred to v0.6+

| Feature | Reason | ETA |
|---------|--------|-----|
| Dynamic JSX tags | AST complexity | v0.6 |
| Forward refs | Hook complexity | v0.6 |
| Error boundaries | Runtime complexity | v0.7 |
| Suspense | Streaming SSR | v0.8 |
| Server streaming | Chunked responses | v0.8 |

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
│  │  (<50ms)            │         │  Islands hydration   │         │
│  └─────────────────────┘         └─────────────────────┘         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Transpilation Pipeline

#### Phase 1: Parsing (Parser)

```
TS/TSX Source
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Custom Recursive Descent Parser              │
│                                                                  │
│  Tokenizer → AST (Concrete) → HIR (Abstract)                  │
│                                                                  │
│  Supports:                                                       │
│  - Full TypeScript syntax (types stripped to HIR)              │
│  - JSX with component detection                                 │
│  - All Fresh route patterns                                     │
│  - Destructuring patterns                                        │
│  - Async/await                                                  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
HIR Module
```

#### Phase 2: Semantic Analysis (Analyzer)

```
HIR Module
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Semantic Analyzer                         │
│                                                                  │
│  1. Classify module type:                                       │
│     - Route file (routes/)                                       │
│     - Island (islands/)                                          │
│     - Layout (_layout.tsx)                                      │
│     - Middleware (_middleware.ts)                               │
│     - Component (components/)                                     │
│                                                                  │
│  2. Extract symbols:                                            │
│     - Named exports                                              │
│     - Default exports (page components)                         │
│     - Handler exports                                            │
│     - Hook usage                                                 │
│     - Island markers                                             │
│                                                                  │
│  3. Validate:                                                    │
│     - No class components                                        │
│     - No dynamic imports                                        │
│     - Supported patterns only                                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
Validated HIR Module
```

#### Phase 3: Code Generation (Codegen)

```
Validated HIR
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Rust Code Generator                        │
│                                                                  │
│  ┌──────────────────┐  ┌──────────────────┐                   │
│  │   JSX Transform  │  │  Hook Transform  │                   │
│  │                  │  │                  │                   │
│  │ <Counter />      │  │ useState(0)      │                   │
│  │       ↓          │  │       ↓          │                   │
│  │ html!(Counter()) │  │ signal(0)       │                   │
│  └──────────────────┘  └──────────────────┘                   │
│                                                                  │
│  ┌──────────────────┐  ┌──────────────────┐                   │
│  │  Route Transform │  │ Island Transform │                   │
│  │                  │  │                  │                   │
│  │ Handler export   │  │ islands/         │                   │
│  │       ↓          │  │       ↓          │                   │
│  │ axum::handler    │  │ HydrationMarker  │                   │
│  └──────────────────┘  └──────────────────┘                   │
│                                                                  │
│  Output: Valid, typed Rust source code                         │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
Rust Source Code
```

### 2.3 Runtime Architecture

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

### 2.4 Islands Architecture

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
│  │       <div data-island="Counter" data-id="island-1"      │   │
│  │         data-props='{"initial":0}'>                      │   │
│  │         <p>Count: 5</p>         ← SSR placeholder        │   │
│  │       </div>                                              │   │
│  │     </main>                                                │   │
│  │     <script>window.__ISLAND_MANIFEST__ = [...];</script> │   │
│  │   </body>                                                 │   │
│  │ </html>                                                   │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Hydration:                                                      │
│  1. Parse island manifest on DOMContentLoaded                   │
│  2. For each island with Visible strategy:                      │
│     a. IntersectionObserver triggers when visible               │
│     b. Load island bundle (lazy)                               │
│     c. Hydrate with SSR props                                   │
│     d. Mark as hydrated                                         │
│                                                                  │
│  Hydration Modes:                                                │
│  - Eager: Immediate on page load                               │
│  - Visible: IntersectionObserver (default)                       │
│  - Idle: requestIdleCallback                                   │
│  - Manual: On explicit trigger                                  │
│  - Static: Never hydrate                                        │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Part III: Development vs Production Modes

### 3.1 Development Mode

**Start**: `runts dev`

**Flow**:
```
1. Scan project structure
2. Pre-load all TS/TSX modules to HIR (in-memory)
3. Build route table from file paths
4. Start file watcher (notify)
5. Start Axum server with HIR interpreter
6. Serve with instant hot-reload
```

**Request Flow**:
```
Request → Route Match → Load HIR (cached) → Execute Handler
    → Call Hooks → Render Component → Compose Layouts
    → Inject Island Markers → HTML Response
```

**Hot Reload**:
- File change → Invalidate cache → Re-parse → Broadcast SSE → Browser refresh
- **Target**: <50ms from file save to visible update

**HIR Interpreter Capabilities**:
- Full TS/TSX expression evaluation
- Preact hooks (useState, useEffect, etc.)
- Signal system
- Component rendering to HTML
- Island detection and placeholder injection

### 3.2 Production Mode

**Build**: `runts build`

**Phase 1: Transpile**
```bash
for each TS/TSX file:
    1. Parse → HIR
    2. Analyze → validated HIR
    3. Generate → Rust source
    4. Write to src/gen/
```

**Phase 2: Compile**
```bash
cargo build --release
    ├── LTO enabled
    ├── single codegen unit
    ├── panic = abort
    └── static linking
```

**Output**:
- Single static binary (<2MB)
- No external dependencies
- Embeddable assets
- Cross-compilation support

---

## Part IV: File Structure

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

## Part V: Type Mappings

| TypeScript | Rust | Notes |
|------------|------|-------|
| `string` | `String` | |
| `number` | `f64` | Integer types preserved |
| `boolean` | `bool` | |
| `null` | `Option<T>::None` | |
| `undefined` | `()` | Unit type |
| `T[]` | `Vec<T>` | |
| `{ a: T }` | `struct { a: T }` | Named structs |
| `T \| null` | `Option<T>` | Nullable |
| `Record<K,V>` | `HashMap<K, V>` | |
| `Promise<T>` | `impl Future<Output = T>` | |
| `PageProps<P>` | `PageProps<P>` | Preserved |
| `HandlerContext` | `HandlerContext` | Full context |
| `JSX.Element` | `VNode` | Virtual DOM node |
| `React.ReactNode` | `VNode` | Renderable node |

---

## Part VI: Performance Targets

| Metric | Target | v0.4 Achieved | Notes |
|--------|--------|---------------|-------|
| **Binary size** | <2MB | ~2.6MB | LTO + strip helps |
| **Memory (baseline)** | <3MB RSS | ~2.8MB | Runtime only |
| **Cold start** | <5ms | <10ms | HTTP server |
| **Hot request** | <0.5ms | <1ms | SSR throughput |
| **SSR throughput** | >100k req/s | >50k | Optimizations pending |
| **Dev HMR** | <50ms | <20ms | HIR cache hit |

### Trade-off Decisions

| Decision | Chosen | Rationale |
|----------|--------|-----------|
| Parser | Custom recursive descent | Control subset, no dep, fast |
| Runtime | Rust-only | No JS engine, max perf |
| Reactivity | Signals + VDOM | Fine-grained + simple |
| Hydration | Islands | Minimal JS, max perf |
| Codegen | In-memory | Fast builds, no temp files |
| Async runtime | Tokio | Battle-tested, async/await |
| HTTP server | Axum | Type-safe, tower integration |
| Hot reload | HIR cache | <50ms without recompile |

---

## Part VII: Roadmap

### v0.5.0 - MVP Completion (Current)

| Feature | Status | Description |
|---------|--------|-------------|
| Route Handler Exports | ✅ | `export const handler = { GET, POST }` |
| Layout Composition | ✅ | Proper `_layout.tsx` rendering |
| Middleware Pipeline | ✅ | Connect to Axum tower |
| Page Data | ✅ | Handler `ctx.render({ data })` |
| Client Island JS | ✅ | Generate minimal hydration bundles |
| Error Pages | ✅ | `_404.tsx`, `_500.tsx` |
| State Sharing | ✅ | `ctx.state` between middleware/handlers |

### v0.6.0 - Feature Complete

| Feature | Priority | Description |
|---------|----------|-------------|
| Dynamic JSX tags | P1 | `<{tagName} />` syntax |
| Forward refs | P1 | `forwardRef` component |
| Enhanced hooks | P1 | `useImperativeHandle`, `useLayoutEffect` |
| Asset pipeline | P2 | CSS/JS bundling |
| Image optimization | P3 | Built-in image handling |

### v1.0.0 - Production Ready

| Feature | Priority | Description |
|---------|----------|-------------|
| Production HMR | P1 | Fine-grained module updates |
| Edge deployment | P2 | WASM target (optional) |
| API Routes | P2 | Full REST API support |
| Database integration | P2 | Prisma/Drizzle ORM |
| Testing utilities | P2 | Component testing helpers |

---

## Appendix A: Route File Conventions

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
routes/_500.tsx          → 500 page
```

---

## Appendix B: Island Conventions

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

## Appendix C: Middleware Conventions

```typescript
// routes/_middleware.ts
import { FreshContext } from "$fresh/server";

interface State {
  user?: {
    id: string;
    name: string;
  };
}

export async function handler(
  req: Request,
  ctx: FreshContext<State>
) {
  // Check for auth cookie
  const cookie = req.headers.get("cookie");
  
  if (cookie?.includes("session")) {
    ctx.state.user = {
      id: "123",
      name: "Demo User"
    };
  }
  
  // Continue to handler
  const resp = await ctx.next();
  
  // Add response headers
  resp.headers.set("X-Custom-Header", "value");
  
  return resp;
}
```

---

## Appendix D: Handler Conventions

```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server";

interface Post {
  title: string;
  content: string;
}

// Handler for GET requests
export const handler = {
  GET: async (req: Request, ctx: PageProps<{ slug: string }>) => {
    const { slug } = ctx.params;
    
    // Fetch post data
    const post = await getPost(slug);
    
    if (!post) {
      return new Response("Not Found", { status: 404 });
    }
    
    // Return rendered page with data
    return ctx.render({ post });
  }
};

interface Data {
  post: Post;
}

export default function BlogPost({ data }: PageProps<Data>) {
  const { post } = data;
  
  return (
    <article>
      <h1>{post.title}</h1>
      <div>{post.content}</div>
    </article>
  );
}
```

---

## Appendix E: Error Codes

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

## Appendix F: Configuration

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
      "optLevel": "z"
    }
  }
}
```

---

*Document Version: 0.5.0*  
*Last Updated: 2026-05-26*
