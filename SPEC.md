# runts - Specification v0.2.0

## Executive Summary

**runts** is a Fresh/Preact-compatible TypeScript framework that compiles to native Rust binaries. It provides zero external JS runtime (no V8, Deno, or Wasm JS engines) while maintaining full Fresh/Preact API compatibility.

### Key Properties

| Property | Value |
|----------|-------|
| **Approach** | TS/TSX → HIR → In-memory Rust codegen → Native binary |
| **Runtime** | Pure Rust (Axum + custom Preact-compatible runtime) |
| **Dev Mode** | HIR interpreter (no Rust recompilation, <100ms hot-reload) |
| **Production** | Full static compilation, single binary |
| **Binary Size Target** | <500KB (with full HTTP server, SSR, routing) |
| **Cold Start** | <10ms (embedded HTTP server) |
| **Memory Baseline** | <2MB RSS |

---

## Part I: Supported TypeScript/TSX Subset

### 1.1 Design Principles

1. **95%+ Coverage First**: Support common patterns, defer edge cases
2. **Predictable Transpilation**: No runtime reflection, pure codegen
3. **Type Safety**: Emit typed Rust, leverage Rust's compiler
4. **Minimal Runtime**: Runtime helpers kept to strict minimum

### 1.2 Supported Features

#### ✅ Language Features

| Feature | Syntax | Notes |
|---------|--------|-------|
| Variables | `const`, `let`, `var` | All scopes supported |
| Functions | `function`, arrow functions, generators | Async fully supported |
| Types | Primitives, unions, intersections, generics | Structural typing |
| Interfaces | `interface` | Converted to Rust structs |
| Type aliases | `type X = ...` | Newtype patterns |
| Enums | `enum` | Converted to Rust enums |
| Classes | `class` | **Islands only**, compiled to Rust structs |
| Destructuring | Object & array patterns | Desugared to individual bindings |
| Spread | `...expr` | Arrays and objects |
| Template literals | `` `hello ${x}` `` | String interpolation |
| Optional chaining | `a?.b?.c` | Null-safe access |
| Nullish coalescing | `a ?? b` | Default values |
| Type assertions | `expr as Type` | Stripped at codegen |

#### ✅ JSX/TSX Support

| Feature | Example | Notes |
|---------|---------|-------|
| Elements | `<div>...</div>` | HTML and SVG |
| Components | `<Counter />` | PascalCase = component |
| Fragments | `<>...</>` | Rendered inline |
| Props | `prop={value}` | CamelCase attr mapping |
| Events | `onClick={handler}` | Auto snake_case |
| Spread | `<div {...props} />` | Merged into attrs |
| Children | `<Parent>{child}</Parent>` | Via `children` prop |
| Conditional | `{condition && <X />}` | Boolean short-circuit |
| Loops | `{items.map(x => <X />)}` | Via array methods |

#### ✅ Preact Hooks (Islands)

| Hook | Status | Notes |
|------|--------|-------|
| `useState` | ✅ | Full |
| `useEffect` | ✅ | SSR-safe (no-op) |
| `useRef` | ✅ | Via `Ref<T>` wrapper |
| `useMemo` | ✅ | Basic memoization |
| `useCallback` | ✅ | Function memoization |
| `useReducer` | ✅ | Full |
| `useContext` | ✅ | Context provider pattern |
| `useSignal` | ✅ | Preact Signals compatible |
| `useComputed` | ✅ | Derived signals |
| `useSignalEffect` | ✅ | Signal effects |

#### ✅ Fresh-Specific

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ | `routes/**/*.tsx` |
| Route patterns | ✅ | Static, param, catch-all |
| Layouts | ✅ | `_layout.tsx` inheritance |
| Middleware | ✅ | `_middleware.ts` |
| Islands | ✅ | `islands/**/*.tsx` |
| `PageProps` | ✅ | Typed route params |
| `HandlerContext` | ✅ | Full context access |
| `Handler` export | ✅ | Route handlers |
| `Default` export | ✅ | Page components |

### 1.3 Explicitly Excluded Features

#### ❌ Not Supported (MVP)

| Feature | Reason | Workaround |
|---------|--------|------------|
| `with` statement | Not in Rust | Destructure explicitly |
| `eval` | Security/Codegen | N/A |
| Dynamic `import()` | Requires bundler | Static imports only |
| `require()` | CommonJS | ES modules only |
| Decorators (`@`) | Stage-2 proposal | Component attrs only |
| Namespace (`namespace`) | Complex codegen | Use ES modules |
| Declare module | Complex | Inline types |
| JSDoc types | Redundant | TypeScript types |
| Conditional types | Complex inference | Explicit union types |
| Template literal types | Complex | String concatenation |
| `infer` keyword | Complex inference | Manual types |
| Recursive types | Infinite codegen | Explicit base types |

#### ❌ Islands Architecture Exclusions

| Feature | Reason | Workaround |
|---------|--------|------------|
| Server-only code in islands | Security | Move to routes/lib |
| Direct DOM access | SSR incompatible | Use refs + effects |
| `window`/`document` | SSR incompatible | Check `IS_BROWSER` |
| Web APIs | Not polyfilled | Use server-side APIs |
| Real-time connections | Complexity | Use WebSocket handler |

#### ❌ Not Planned (Future Consideration)

| Feature | Complexity | Priority |
|---------|------------|----------|
| Class components | Runtime complexity | Functional only |
| Error boundaries | Error handling | Try/catch in handlers |
| Suspense | Async complexity | Streaming responses |
| Server Components | Architecture shift | Fresh 2.0 |

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

### 2.2 Transpilation Pipeline

#### 2.2.1 Parser (TSX → HIR)

The parser handles a **TypeScript/TSX subset** using a custom recursive descent parser:

```rust
// Parser output: High-Level IR (HIR)
pub struct Module {
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}

pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}
```

**Parser Capabilities:**
- Full TypeScript syntax (subset)
- JSX parsing with semantic element detection
- Type annotations preserved for codegen
- Source location tracking for error reporting

**Key Decisions:**
- No external parser dependency (swc/Deno)
- Custom parser for control over subset
- Focus on correct Fresh patterns

#### 2.2.2 Semantic Analyzer

The analyzer performs validation and enrichment:

```rust
pub struct AnalysisContext {
    // File classification
    pub is_island: bool,
    pub is_route: bool,
    pub is_middleware: bool,
    pub is_layout: bool,
    
    // Symbol tables
    pub variables: HashMap<String, Type>,
    pub functions: HashMap<String, FunctionSig>,
    pub components: Vec<ComponentInfo>,
    
    // Diagnostics
    pub errors: Vec<Diagnostic>,
}
```

**Analysis Steps:**
1. **Classification**: Determine file type (island/route/component/middleware)
2. **Symbol Resolution**: Map identifiers to declarations
3. **Type Checking**: Verify type compatibility
4. **Hook Detection**: Identify Preact hook usage
5. **Component Detection**: Identify component functions

**Validation Rules:**
- Islands cannot import routes
- Routes cannot have client-side state (hooks)
- Components must be PascalCase
- Handlers must be async

#### 2.2.3 Code Generator (HIR → Rust)

The code generator produces idiomatic Rust:

```rust
pub struct CodeGenerator {
    // Imports management
    imports: Vec<String>,
    
    // Component registry
    components: Vec<ComponentGen>,
    
    // Route table
    routes: Vec<RouteGen>,
}
```

**Key Transformations:**

| TypeScript | Rust |
|------------|------|
| `interface Props` | `#[derive(Serialize, Deserialize)] pub struct Props` |
| `function Component(props)` | `#[component] pub fn component(props: Props) -> VNode` |
| `<div class="x">` | `html!(<div class_name="x"></div>)` |
| `onClick={fn}` | `on_click=Box::new(fn)` |
| `{condition && <X/>}` | `if condition { Some(html!(<X/>)) } else { None }` |
| `items.map(x => ...)` | `items.iter().map(\|x\| ...).collect::<Vec<_>>()` |
| `useState(0)` | `let (state, set_state) = use_state(\|\| 0)` |

**html! Macro Expansion:**

```rust
// TypeScript
<div className="container">
  <h1>Hello {name}</h1>
  <Button onClick={handleClick}>Click</Button>
</div>

// Generated Rust
html!(<div class_name="container">
  <h1>"Hello " {name}</h1>
  <Button on_click=Box::new(handle_click)>"Click"</Button>
</div>)
```

### 2.3 Runtime Architecture

#### 2.3.1 Server Runtime

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
│  │  │  Renderer   │  │  Placeholder │  │  Composer       │   │   │
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

#### 2.3.2 Islands Architecture

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
│  │       <div data-island="Counter" data-id="abc123">       │   │
│  │         <p>Count: 5</p>         ← SSR placeholder        │   │
│  │         <button>-</button>                                 │   │
│  │         <button>+</button>                                 │   │
│  │       </div>                                              │   │
│  │     </main>                                                │   │
│  │     <script>window.__RUNTS_ISLANDS__ = [...];</script>   │   │
│  │   </body>                                                 │   │
│  │ </html>                                                   │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Client Hydration:                                              │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ 1. Parse islands manifest                                 │   │
│  │ 2. Register island components                            │   │
│  │ 3. For each island:                                      │   │
│  │    a. Match SSR HTML by data-id                         │   │
│  │    b. Attach event listeners                            │   │
│  │    c. Restore component state from data props           │   │
│  │ 4. Mark as hydrated                                     │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Hydration Modes:                                                │
│  - Eager: Immediate on page load                               │
│  - Lazy: On intersection (IntersectionObserver)                 │
│  - Interaction: On click/focus/hover                            │
│  - Visible: On entering viewport                               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### 2.3.3 Signal System

```
┌─────────────────────────────────────────────────────────────────┐
│                  Fine-Grained Reactivity                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Signal<T> ─────────────────────────────────────────────┐      │
│     │                                                      │      │
│     ├── value: Arc<RwLock<T>>    (reactive value)        │      │
│     ├── subscribers: Vec<Callback>  (effect list)        │      │
│     └── next_id: AtomicUsize      (unique IDs)           │      │
│                                                              │      │
│  Computed<T> ───────────────────────────────────────────┐       │
│     │                                                      │       │
│     ├── dependencies: Vec<SignalId>  (tracked)          │       │
│     ├── dirty: AtomicBool           (recompute flag)     │       │
│     └── value: Signal<T>           (cached result)      │       │
│                                                              │       │
│  Effect ───────────────────────────────────────────────┐       │
│     │                                                      │       │
│     ├── dependencies: Vec<SignalId>  (auto-tracked)     │       │
│     └── cleanup: Fn()                (on dispose)        │       │
│                                                              │       │
│  Example:                                                    │       │
│  ┌─────────────────────────────────────────────────────┐ │       │
│  │ const count = signal(0);                             │ │       │
│  │ const doubled = computed(() => count.value * 2);     │ │       │
│  │ useEffect(() => console.log(doubled.value));         │ │       │
│  │                                                        │ │       │
│  │ // When count.value = 5:                             │ │       │
│  │ //   doubled.value = 10 (auto-computed)              │ │       │
│  │ //   console.log fires (auto-subscribed)             │ │       │
│  └─────────────────────────────────────────────────────┘ │       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.4 File Structure

```
runts-project/
├── routes/                      # File-based routing
│   ├── _middleware.ts          # Global middleware
│   ├── _layout.tsx             # Root layout
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
├── deno.json                   # TypeScript config (compatibility)
├── runts.config.json           # Runts configuration
├── Cargo.toml                  # Rust dependencies
└── src/
    ├── main.rs                 # Entry point (generated)
    ├── lib.rs                  # Library (generated)
    ├── routes.rs               # Route table (generated)
    ├── islands.rs              # Islands manifest (generated)
    └── components.rs           # Components (generated)
```

---

## Part III: Development vs Production Modes

### 3.1 Development Mode

```
┌─────────────────────────────────────────────────────────────────┐
│                    Development Mode                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Start: `runts dev`                                              │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │ 1. Scan project structure                                │   │
│  │ 2. Pre-load all TS/TSX modules                          │   │
│  │ 3. Build route table                                    │   │
│  │ 4. Start file watcher (notify)                          │   │
│  │ 5. Start Axum server                                   │   │
│  │ 6. Serve with HIR interpreter                          │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Request Flow:                                                  │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │  Request: GET /blog/my-post                              │   │
│  │       │                                                  │   │
│  │       ▼                                                  │   │
│  │  Route Table Match                                       │   │
│  │       │                                                  │   │
│  │       ▼                                                  │   │
│  │  Load HIR (cached or fresh)                             │   │
│  │       │                                                  │   │
│  │       ▼                                                  │   │
│  │  Execute Handler via Interpreter                         │   │
│  │       │                                                  │   │
│  │       ├── Parse route params                             │   │
│  │       ├── Execute handler body                          │   │
│  │       ├── Call hooks (useState, etc.)                   │   │
│  │       └── Return page data (JSON)                       │   │
│  │       │                                                  │   │
│  │       ▼                                                  │   │
│  │  Render Default Component                                │   │
│  │       │                                                  │   │
│  │       ├── Build VDOM from JSX                           │   │
│  │       ├── Render to HTML string                         │   │
│  │       ├── Inject island markers                          │   │
│  │       └── Compose with layouts                           │   │
│  │       │                                                  │   │
│  │       ▼                                                  │   │
│  │  Response: Full HTML page                                │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Hot Reload Flow:                                               │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │  File Change (notify event)                               │   │
│  │       │                                                  │   │
│  │       ├── Invalidate module cache                        │   │
│  │       ├── Re-parse changed module to HIR                  │   │
│  │       ├── Update route table (if routes changed)         │   │
│  │       └── Broadcast reload event                         │   │
│  │       │                                                  │   │
│  │       ├── SSE client receives reload                     │   │
│  │       └── Browser fetches fresh page                     │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Performance:                                                    │
│  - File change → HMR notification: <50ms                        │
│  - HMR notification → Page refresh: <100ms                      │
│  - No Rust recompilation needed                                 │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Production Mode

```
┌─────────────────────────────────────────────────────────────────┐
│                    Production Mode                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Build: `runts build`                                           │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │ Phase 1: Transpile                                        │   │
│  │  ┌─────────────────────────────────────────────────────┐  │   │
│  │  │ for each TS/TSX file:                              │  │   │
│  │  │   1. Parse → HIR                                   │  │   │
│  │  │   2. Analyze → validated HIR                       │  │   │
│  │  │   3. Generate → Rust source                       │  │   │
│  │  │   4. Write to src/gen/                            │  │   │
│  │  └─────────────────────────────────────────────────────┘  │   │
│  │                                                          │   │
│  │ Phase 2: Compile                                          │   │
│  │  ┌─────────────────────────────────────────────────────┐  │   │
│  │  │ cargo build --release                              │  │   │
│  │  │   ├── LTO enabled                                  │  │   │
│  │  │   ├── single codegen unit                         │  │   │
│  │  │   ├── panic = abort                               │  │   │
│  │  │   └── static linking                             │  │   │
│  │  └─────────────────────────────────────────────────────┘  │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Runtime:                                                       │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │  Binary Output                                            │   │
│  │  ┌─────────────────────────────────────────────────────┐  │   │
│  │  │ ./target/release/my-app                             │  │   │
│  │  │   ├── Pure Rust (no runtime deps)                  │  │   │
│  │  │   ├── Static HTTP server                           │  │   │
│  │  │   ├── Compiled route handlers                      │  │   │
│  │  │   ├── Compiled components                          │  │   │
│  │  │   └── Embedded client runtime (~30KB)              │  │   │
│  │  └─────────────────────────────────────────────────────┘  │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Execution:                                                     │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │  ./my-app                                               │   │
│  │       │                                                  │   │
│  │       ├── Parse CLI args                                 │   │
│  │       ├── Load config                                    │   │
│  │       ├── Build route tree (compiled)                    │   │
│  │       ├── Start Axum server                              │   │
│  │       └── Serve requests (zero overhead)                │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Part IV: Roadmap

### 4.1 MVP (v0.1.0 - v0.2.0) ✅

**Goal**: Core Fresh compatibility for static content + basic islands

| Feature | Status | Notes |
|---------|--------|-------|
| File-based routing | ✅ | Static routes |
| Route params | ✅ | `[slug].tsx` |
| Route handlers | ✅ | `GET`, `POST`, etc. |
| Layouts | ✅ | `_layout.tsx` |
| Middleware | ✅ | `_middleware.ts` |
| Components | ✅ | Static rendering |
| Islands (basic) | ✅ | useState, events |
| JSX | ✅ | HTML elements + components |
| TS types | ✅ | Interfaces, unions |
| Dev server | ✅ | HIR interpreter |
| Production build | ✅ | Rust codegen |

### 4.2 v0.3.0 - Islands Completion

| Feature | Priority | Description |
|---------|----------|-------------|
| All hooks | P0 | useEffect, useRef, useReducer, etc. |
| Signal integration | P0 | Preact Signals compatibility |
| Context API | P1 | createContext, useContext |
| Island hydration modes | P1 | eager, lazy, visible, interaction |
| HMR for islands | P1 | Partial hydration on change |
| Error boundaries | P2 | Graceful island failures |

### 4.3 v0.4.0 - API Completeness

| Feature | Priority | Description |
|---------|----------|-------------|
| All Fresh handlers | P0 | HEAD, OPTIONS, etc. |
| Fresh `State` | P1 | Middleware state sharing |
| Fresh `MiddlewareHandler` types | P1 | Full type safety |
| `_app.tsx` wrapper | P1 | App-level wrapper |
| `_404.tsx` | P1 | Custom 404 page |
| `_500.tsx` | P2 | Custom error page |

### 4.4 v0.5.0 - Performance & DX

| Feature | Priority | Description |
|---------|----------|-------------|
| SSR streaming | P1 | Incremental HTML output |
| Edge deployment | P1 | WASM target (alternative) |
| Dev toolbar | P2 | Visual HMR feedback |
| Source maps | P2 | Debug generated Rust |
| VSCode extension | P3 | Type checking, completion |

### 4.5 v1.0.0 - Production Ready

| Feature | Priority | Description |
|---------|----------|-------------|
| Full test suite | P0 | Unit + integration |
| Documentation | P0 | Complete Fresh parity |
| Benchmarks | P0 | Performance verification |
| Binary size <500KB | P1 | Optimization pass |
| Memory <2MB baseline | P1 | Memory profiling |
| Production deployments | P1 | Real-world validation |

### 4.6 Future (Post-v1.0)

| Feature | Priority | Description |
|---------|----------|-------------|
| Server Components | P2 | Zero-JS server rendering |
| Asset pipeline | P2 | CSS/JS bundling |
| Image optimization | P3 | Built-in image handling |
| i18n | P3 | Internationalization |

---

## Part V: Performance Targets & Trade-offs

### 5.1 Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Binary size** | <500KB | `ls -lh target/release/binary` |
| **Memory (baseline)** | <2MB RSS | `/usr/bin/time -v` |
| **Cold start** | <10ms | Time to first byte |
| **Hot request** | <1ms | Route handler only |
| **SSR throughput** | >50k req/s | wrk benchmark |
| **Dev HMR** | <100ms | File change → visible update |
| **Build time** | <30s | `runts build` for typical project |

### 5.2 Memory Budget

```
┌─────────────────────────────────────────────────────────────┐
│                    Memory Budget (2MB baseline)              │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Axum stack          │  ~100KB                              │
│  Tower middleware    │  ~50KB                               │
│  Route table         │  ~20KB                               │
│  VDOM buffer         │  ~50KB                               │
│  Client JS runtime   │  ~30KB                               │
│  Static assets       │  (embedded, per-use)                │
│  Working memory      │  ~200KB (typical request)           │
│  Rust global state   │  ~100KB                             │
│  Slop                │  ~1.4MB                              │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 5.3 Trade-off Decisions

| Decision | Chosen | Rationale |
|----------|--------|-----------|
| Parser | Custom | Control subset, no dep |
| Runtime | Rust-only | No JS engine, max perf |
| Reactivity | Signals | Fine-grained, efficient |
| Hydration | Islands | Minimal JS, max perf |
| Codegen | In-memory | Fast builds, no temp files |
| Error handling | Panic=abort | Smaller binary, simpler |
| Async runtime | Tokio | Battle-tested, async/await |
| HTTP server | Axum | Type-safe, tower integration |

### 5.4 Comparison with Alternatives

| Framework | Binary | Memory | Runtime | JS Bundle |
|-----------|--------|--------|---------|-----------|
| **runts** | <500KB | <2MB | Rust-only | <50KB |
| Fresh | N/A | N/A | Deno | <100KB |
| Next.js | N/A | N/A | Node | <200KB |
| SvelteKit | N/A | N/A | Node | <50KB |
| Leptos | <1MB | <2MB | Rust-only | 0KB |
| Dioxus | <5MB | <5MB | Rust-only | 0KB |

---

## Part VI: Component Reference

### 6.1 Supported HTML Elements

All standard HTML5 elements are supported:

```tsx
// Typography
<div>, <span>, <p>, <h1> through <h6>, <a>, <strong>, <em>

// Lists  
<ul>, <ol>, <li>, <dl>, <dt>, <dd>

// Tables
<table>, <thead>, <tbody>, <tfoot>, <tr>, <th>, <td>

// Forms
<form>, <input>, <button>, <textarea>, <select>, <option>
<label>, <fieldset>, <legend>

// Media
<img>, <video>, <audio>, <canvas>, <svg>, <iframe>

// Semantic
<header>, <nav>, <main>, <article>, <section>, <aside>, <footer>
<time>, <address>, <figure>, <figcaption>

// Interactive
<dialog>, <details>, <summary>, <menu>

// Custom
Any valid SVG element
```

### 6.2 Supported Attributes

| Category | Attributes |
|----------|-------------|
| Global | `id`, `class`, `style`, `title`, `lang`, `dir`, `data-*` |
| Events | `onClick`, `onInput`, `onChange`, `onSubmit`, `onKeyDown`, `onKeyUp`, `onFocus`, `onBlur`, `onMouseEnter`, `onMouseLeave`, `onScroll`, `onLoad`, `onError` |
| Form | `type`, `name`, `value`, `checked`, `selected`, `disabled`, `readonly`, `required`, `placeholder`, `autocomplete`, `autofocus`, `multiple` |
| Media | `src`, `alt`, `width`, `height`, `poster`, `controls`, `loop`, `muted` |
| Links | `href`, `target`, `rel`, `download` |
| Meta | `charset`, `content`, `http-equiv`, `name` |
| Aria | `aria-label`, `aria-describedby`, `aria-hidden`, `role` |

### 6.3 Type Mappings

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
| `T \| undefined` | `Option<T>` |
| `Record<K,V>` | `HashMap<K, V>` |
| `Map<K,V>` | `std::collections::HashMap` |
| `Set<T>` | `std::collections::HashSet` |
| `Promise<T>` | `impl Future<Output = T>` |

---

## Part VII: Example Project Structure

### 7.1 Minimal Project

```
my-app/
├── routes/
│   └── index.tsx          # Home page
├── islands/
│   └── Counter.tsx        # Interactive counter
├── runts.config.json
└── Cargo.toml
```

### 7.2 Typical Blog

```
my-blog/
├── routes/
│   ├── _middleware.ts
│   ├── _layout.tsx
│   ├── index.tsx
│   ├── about.tsx
│   └── blog/
│       ├── _layout.tsx
│       ├── index.tsx
│       └── [slug].tsx
├── islands/
│   ├── SearchBox.tsx
│   ├── CommentForm.tsx
│   └── Newsletter.tsx
├── components/
│   ├── Header.tsx
│   ├── Footer.tsx
│   ├── PostCard.tsx
│   └── AuthorBio.tsx
├── lib/
│   ├── posts.ts           # Blog data access
│   └── utils.ts
├── static/
│   ├── favicon.ico
│   └── styles.css
├── runts.config.json
└── Cargo.toml
```

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

## Appendix B: Configuration Reference

```json
{
  "server": {
    "port": 8000,
    "host": "127.0.0.1"
  },
  "islands": {
    "hydration": "lazy",
    "serializer": "json"
  },
  "dev": {
    "port": 8000,
    "open": true,
    "hmr": true
  },
  "watch": {
    "ignored": ["**/node_modules/**", "**/target/**"],
    "include": ["routes/**", "islands/**", "components/**"]
  },
  "build": {
    "target": null,
    "optimization": {
      "lto": true,
      "opt_level": "z"
    }
  }
}
```

---

*Document Version: 0.2.0*  
*Last Updated: 2026-05-26*
