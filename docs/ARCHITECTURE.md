# runts Architecture

> A Fresh/Preact-compatible framework that compiles TypeScript/TSX to native Rust binaries.

---

## 1. Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            User Source Code                                  │
│    routes/*.tsx  islands/*.tsx  components/*.tsx  static/*  runts.config.json│
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         runts Compiler Pipeline                              │
│                                                                              │
│  ┌─────────┐    ┌──────────┐    ┌─────────┐    ┌──────────┐    ┌─────────┐ │
│  │  Parse  │───▶│   HIR    │───▶│ Analyze │───▶│ Transform│───▶│ Codegen │ │
│  │(TS/TSX) │    │(Typed IR)│    │(Seman-  │    │ (Lower)  │    │(Rust src)│ │
│  └─────────┘    └──────────┘    │ tic)    │    └──────────┘    └─────────┘ │
│                                 └─────────┘                                  │
│                                                                              │
│  Development:         HIR ──▶ Interpreter ──▶ Axum Server (zero compile)    │
│  Production:          Rust ──▶ rustc/cargo ──▶ Native Binary                │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Runtime Library (runts-lib)                          │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐           │
│  │  VDOM   │  │ Signals │  │  Hooks  │  │ Islands │  │  Server │           │
│  │ (VNode) │  │(Reactive)│  │(Preact) │  │(Hydrate)│  │ (Axum)  │           │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘  └─────────┘           │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Client Runtime (vanilla JS)                          │
│         Islands hydration: eager / visible / idle / manual                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

1. **Dual-mode execution**: Same TS/TSX source runs in development (HIR interpreter) and production (native binary) with identical semantics.
2. **Fine-grained reactivity**: Preact signals mapped to Leptos-style Rust signals for zero VDOM overhead in reactive paths.
3. **Islands as compile-time boundaries**: Islands are analyzed at parse time; non-island code is fully server-rendered with zero client JS.
4. **No JS runtime**: Parser, analyzer, and runtime are pure Rust. Client JS is generated as static bundles.

---

## 2. Compiler Pipeline

### 2.1 Parser (Recursive Descent)

- **Input**: Raw TS/TSX source files
- **Output**: `Module` AST (Concrete Syntax Tree)
- **Approach**: Hand-written recursive descent with zero dependencies
- **Performance**: ~1MB/s parsing speed (single-threaded)

```rust
pub struct Parser {
    source: String,
    pos: usize,
    tokens: Vec<Token>,
}

impl Parser {
    pub fn parse_module(&mut self) -> Result<Module, ParseError>;
    pub fn parse_jsx_element(&mut self) -> Result<JSXElement, ParseError>;
    pub fn parse_type_annotation(&mut self) -> Result<TypeAnn, ParseError>;
}
```

**Why hand-written instead of swc/tree-sitter?**
- We need precise control over error messages for the subset
- No dependency on JS/WASM runtime
- Smaller binary size (~50KB vs ~5MB for swc)
- Easier to extend for Fresh-specific constructs

### 2.2 HIR (High-level Intermediate Representation)

HIR is a typed AST that normalizes all TS constructs to a core set:

```rust
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    ArrowFn { params: Vec<Param>, body: Block },
    JSXElement(JSXElement),
    Await(Box<Expr>),
    Member { obj: Box<Expr>, prop: String },
    Array(Vec<Expr>),
    Object(Vec<ObjectProp>),
    // ...
}

pub enum Stmt {
    Let { name: String, init: Expr, mutable: bool },
    Expr(Expr),
    Return(Option<Expr>),
    If { cond: Expr, then: Block, else_: Option<Block> },
    For { init: Expr, cond: Expr, update: Expr, body: Block },
    // ...
}

pub struct Module {
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub declarations: Vec<Decl>,
    pub is_route: bool,
    pub is_island: bool,
    pub is_layout: bool,
}
```

HIR bridges the gap between TS's loose syntax and Rust's strict semantics. All type annotations are preserved but not checked — Rust's type checker validates the generated code.

### 2.3 Semantic Analyzer

The analyzer performs:

1. **Route detection**: Scans `routes/` files for `handler` exports and default component exports
2. **Island detection**: Marks `islands/` files as interactive; extracts props interface
3. **Hook validation**: Ensures hooks are called at top level (Rules of Hooks)
4. **Type extraction**: Extracts interfaces for props, handler data, and route params
5. **Import resolution**: Maps `$fresh/server.ts` to runtime prelude
6. **JSX attribute validation**: Ensures event handlers exist in the supported set

```rust
pub struct Analyzer {
    route_patterns: Vec<RoutePattern>,
    island_registry: IslandRegistry,
    diagnostics: Vec<Diagnostic>,
}

impl Analyzer {
    pub fn analyze(&mut self, module: &Module) -> Result<(), Vec<Diagnostic>>;
    pub fn extract_route_info(&self, module: &Module) -> Option<RouteInfo>;
    pub fn extract_island_info(&self, module: &Module) -> Option<IslandInfo>;
}
```

### 2.4 Code Generator

Converts HIR → Rust source code. This is the most complex phase.

```rust
pub struct CodeGenerator {
    indent: usize,
    output: String,
    generate_handlers: bool,
    generate_island_wrappers: bool,
}

impl CodeGenerator {
    pub fn generate_module(&mut self, module: &Module) -> Result<String, GenError>;
    pub fn generate_expr(&mut self, expr: &Expr) -> String;
    pub fn generate_jsx(&mut self, jsx: &JSXElement) -> String;
    pub fn generate_handler(&mut self, route: &RouteInfo) -> String;
}
```

#### Key transformations:

| TypeScript | Rust |
|-----------|------|
| `let x = 5` | `let x = 5;` (immutable) |
| `let x = 5; x = 6` | `let mut x = 5; x = 6;` |
| `const [a, b] = arr` | `let (a, b) = (arr[0].clone(), arr[1].clone());` |
| `function f() {}` | `pub fn f() {}` |
| `async function f() {}` | `pub async fn f() {}` |
| `obj.map(x => x * 2)` | `obj.iter().map(\|x\| x * 2).collect::<Vec<_>>() ` |
| `{...obj, a: 1}` | `{ ..obj, a: 1 }` |
| `interface P { a: string }` | `#[derive(Serialize, Deserialize)]\npub struct P { a: String }` |

#### JSX → `html!` macro:

```rust
// TypeScript JSX
<div className="foo" onClick={handleClick}>
  {children}
</div>

// Generated Rust
html!(
    <div class_name = "foo" on_click = {handleClick}>
        {children}
    </div>
)
```

### 2.5 Route Generator

File-based routing with Fresh conventions:

```
routes/
  index.tsx        →  GET /
  about.tsx        →  GET /about
  blog/
    index.tsx      →  GET /blog
    [slug].tsx     →  GET /blog/:slug
    _layout.tsx    →  Layout wrapper for /blog/*
  _middleware.ts   →  Global middleware
  api/
    users.ts       →  GET /api/users
```

Route generation produces:
1. Route table with regex patterns
2. Handler dispatch functions
3. Param extraction logic
4. Middleware chain ordering

### 2.6 Middleware Generator

Extracts middleware from `_middleware.ts` files and generates pipeline code:

```rust
// Generated middleware pipeline
pub async fn middleware_pipeline(
    req: Request,
    route: &str,
    global: Vec<MiddlewareFn>,
    route_specific: Vec<MiddlewareFn>,
    handler: HandlerFn,
) -> Response {
    // Execute global → route → handler
}
```

---

## 3. Runtime Architecture

### 3.1 Virtual DOM (VDOM)

Server-side VDOM for SSR and islands:

```rust
pub enum VNode {
    Text(String),
    Element {
        tag: &'static str,
        attrs: HashMap<String, String>,
        children: Vec<VNode>,
        key: Option<String>,
    },
    Component {
        render: Box<dyn Fn() -> VNode>,
        props: serde_json::Value,
        key: Option<String>,
    },
    Fragment(Vec<VNode>),
    Island {
        name: String,
        props: serde_json::Value,
        children: Vec<VNode>,
        hydration: HydrationMode,
    },
}

impl VNode {
    pub fn to_html(&self) -> String;
    pub fn to_html_escaped(&self) -> String;
}
```

**Key insight**: VDOM is only used for initial SSR. On the client, islands use fine-grained signals directly — no VDOM diffing for reactive updates.

### 3.2 Signals (Fine-grained Reactivity)

Preact signals reimplemented in Rust using a graph-based dependency tracker:

```rust
pub struct Signal<T> {
    value: RefCell<T>,
    version: Cell<u64>,
    subscribers: RefCell<Vec<Weak<Effect>>>,
}

impl<T: Clone> Signal<T> {
    pub fn new(value: T) -> Self;
    pub fn get(&self) -> T;      // Subscribe + read
    pub fn set(&self, value: T);  // Notify subscribers
    pub fn peek(&self) -> T;     // Read without subscribe
}

pub struct Computed<T> {
    compute: Box<dyn Fn() -> T>,
    cache: RefCell<Option<T>>,
    version: Cell<u64>,
}

pub struct Effect {
    execute: Box<dyn Fn()>,
    dependencies: RefCell<Vec<Weak<dyn SignalBase>>>,
}
```

**Algorithm**: Similar to Preact signals but using `Rc`/`Weak` instead of a linked-list approach. Batched updates via a global transaction queue.

**Performance**: Signal reads are O(1) (cached). Signal writes are O(n) where n = subscriber count (typically small).

### 3.3 Hooks

Hooks are implemented as a thread-local call stack, identical to React's hooks model:

```rust
thread_local! {
    static HOOK_STATE: RefCell<Vec<HookEntry>> = RefCell::new(Vec::new());
    static HOOK_INDEX: Cell<usize> = Cell::new(0);
}

pub fn use_state<T: Clone>(initial: impl FnOnce() -> T) -> (T, Setter<T>) {
    HOOK_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let idx = HOOK_INDEX.get();
        if idx >= state.len() {
            state.push(HookEntry::State(initial()));
        }
        let value = state[idx].as_state().clone();
        let setter = Setter { index: idx };
        HOOK_INDEX.set(idx + 1);
        (value, setter)
    })
}
```

**Server-side**: Hooks execute once during SSR and produce initial values.
**Client-side**: Hooks execute during island hydration and establish reactive subscriptions.

### 3.4 Islands Architecture

Islands are the boundary between server-rendered HTML and client-side interactivity:

```rust
pub struct IslandInstance {
    pub name: String,
    pub id: String,
    pub props: serde_json::Value,
    pub hydration: HydrationMode,
    pub placeholder_html: String,
}

pub enum HydrationMode {
    Eager,      // Hydrate immediately
    Visible,    // IntersectionObserver
    Idle,       // requestIdleCallback
    Manual,     // Wait for explicit trigger
}
```

#### SSR Phase:
1. Render island component to VNode
2. Generate `<div data-island="Name" data-props="{...}">`
3. Serialize props as JSON in `data-props`
4. Include island JS bundle reference

#### Client Phase:
1. `runtime.js` scans `document.querySelectorAll('[data-island]')`
2. Groups islands by hydration strategy
3. Hydrates according to strategy
4. Re-renders island with reactive signals

#### Island Bundle Generation:

Each island gets its own minimal JS bundle containing:
- The island component function
- Required hooks (useState, useEffect, etc.)
- Signal runtime
- Event delegation

Bundle size per island: ~2-5KB (minified + gzipped) for simple islands.

### 3.5 Server (Axum/Tower)

Production server uses Axum with Tower middleware:

```rust
pub fn create_router(routes: RouteTable) -> Router {
    let mut router = Router::new();
    
    for route in routes.iter() {
        let handler = route.handler.clone();
        router = router.route(
            &route.pattern,
            get(handler),
        );
    }
    
    router
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}
```

#### Request lifecycle:

```
Request
  → Tower middleware (logging, compression, CORS)
  → Route matcher (regex-based)
  → Param extraction
  → Middleware pipeline (global → route-specific)
  → Handler / Component renderer
  → SSR (if page route)
  → Response
```

---

## 4. Development Mode

### 4.1 HIR Interpreter

In development, TS/TSX is parsed to HIR and executed directly — no Rust compilation:

```rust
pub struct Interpreter {
    modules: HashMap<String, Module>,
    globals: HashMap<String, Value>,
    route_table: RouteTable,
}

impl Interpreter {
    pub fn load_file(&mut self, path: &Path, source: &str) -> Result<()>;
    pub fn eval_module(&mut self, name: &str) -> Result<Value>;
    pub fn handle_request(&self, req: Request) -> Result<Response>;
}
```

The interpreter supports:
- Full expression evaluation
- JSX rendering to HTML strings
- Hook execution (stateful!)
- Signal reactivity
- Async/await
- Module imports (resolved to in-memory modules)

**Why an interpreter instead of incremental compilation?**
- Compilation takes 5-30s even for small projects
- Interpreter reloads in <100ms
- Same HIR is used for both modes, guaranteeing parity
- Allows interactive debugging at the HIR level

### 4.2 Hot Reload

```
File change detected
  → Re-parse affected module (~10ms)
  → Update in-memory module table
  → Broadcast reload event to browser via WebSocket
  → Browser reloads or HMR patches
```

The dev server includes a small WebSocket endpoint that pushes reload events. Client JS auto-reconnects.

### 4.3 Dev Server Configuration

```json
{
  "dev": {
    "port": 3000,
    "host": "0.0.0.0",
    "hotReload": true,
    "openBrowser": false,
    "proxy": {
      "/api": "http://localhost:8080"
    }
  }
}
```

---

## 5. Production Build

### 5.1 Build Pipeline

```
Source TS/TSX files
  → Parse → HIR
  → Analyze (routes, islands, types)
  → Generate Rust source (src/gen/)
  → Generate route table (src/routes.rs)
  → Generate island registry (src/islands.rs)
  → Generate main.rs (Axum server setup)
  → cargo build --release
  → Native binary
```

### 5.2 Code Generation Structure

```
my-project/
├── routes/
│   ├── index.tsx
│   └── about.tsx
├── islands/
│   └── Counter.tsx
└── src/
    ├── main.rs              # Entry point (generated)
    ├── routes.rs            # Route table (generated)
    ├── islands.rs           # Island registry (generated)
    ├── gen/
    │   ├── mod.rs
    │   ├── index.rs         # routes/index.tsx → Rust
    │   ├── about.rs         # routes/about.tsx → Rust
    │   └── islands/
    │       └── counter.rs   # islands/Counter.tsx → Rust
    └── components.rs        # Static components (generated)
```

### 5.3 Optimization Pipeline

1. **Dead code elimination**: Islands are tree-shaken; only used code is bundled
2. **Link-time optimization (LTO)**: Full LTO for minimal binary size
3. **Code generation units = 1**: Slower compile, faster runtime
4. **Strip symbols**: `strip = true` in Cargo.toml
5. **Panic = abort**: No unwinding overhead

### 5.4 Client Bundle Generation

Islands requiring client JS are compiled to a separate JS bundle:

```
Islands source
  → Parse → HIR
  → Extract island component
  → Generate vanilla JS (no React/Preact runtime!)
  → Bundle with esbuild (or runts-native bundler)
  → Output: dist/islands/{name}.js
```

The client JS is intentionally minimal — it contains only:
- The island component logic
- Hook implementations (subset)
- Signal runtime
- Event delegation
- Hydration bootstrap

No virtual DOM. No diffing. Direct DOM manipulation via signals.

---

## 6. Memory Model

### 6.1 Server-side

- **Request-scoped**: Most values are created and dropped per request
- **Static**: Routes, components, and config are compiled into `.rodata`
- **Shared**: Island registries and signal graphs use `Arc` for safe sharing

### 6.2 Client-side (Islands)

- **Signal graph**: `Rc`/`Weak` references form a DAG
- **Effects**: Owned by the island instance; dropped on unmount
- **DOM refs**: `Option<Element>` updated during hydration

### 6.3 Thread Safety

- Server: Tokio tasks handle requests concurrently; components are `Send + Sync`
- Signals: `Signal<T>` requires `T: Send + Sync` for cross-task use
- Hooks: Thread-local in interpreter; `Send` in compiled code

---

## 7. Error Handling Strategy

### 7.1 Compile-time Errors

| Error Code | Description | Recovery |
|------------|-------------|----------|
| R0001 | Unsupported feature | Must fix |
| R0002 | Parse error | Must fix |
| R0010 | Type mismatch | Must fix (propagated from rustc) |
| R0020 | Hook rule violation | Must fix |
| R0030 | Missing island prop | Must fix |
| R0040 | Invalid route pattern | Must fix |
| R0050 | Import not found | Must fix or add alias |

### 7.2 Runtime Errors

| Error | Server Behavior | Client Behavior |
|-------|----------------|-----------------|
| Panic in handler | 500 response, log traceback | N/A |
| Panic in island | N/A | Island shows error boundary, logs to console |
| Missing island bundle | N/A | SSR placeholder remains, console warning |
| Hydration mismatch | N/A | Full re-render (graceful degradation) |

### 7.3 Error Pages

- `routes/_404.tsx`: Custom 404 page
- `routes/_500.tsx`: Custom 500 page
- Default fallbacks provided if not present

---

## 8. Security Model

1. **XSS Prevention**: All JSX expressions are HTML-escaped by default. `dangerouslySetInnerHTML` requires explicit opt-in.
2. **CSRF Protection**: Built-in CSRF token generation for form submissions.
3. **CSP Headers**: Auto-generated Content-Security-Policy for island scripts.
4. **No eval**: Dynamic code execution is impossible by design.
5. **Request validation**: Route params are validated against patterns at compile time.

---

## 9. Plugin Architecture (Future)

```rust
pub trait Plugin {
    fn name(&self) -> &str;
    fn transform_hir(&self, module: &mut Module) -> Result<()>;
    fn generate_client_code(&self, island: &Island) -> Option<String>;
}
```

Plugins will be compiled as dynamic libraries (`.so`/`.dylib`) and loaded at build time.

---

## 10. File Structure

```
runts/
├── Cargo.toml                    # Workspace definition
├── src/
│   ├── main.rs                   # CLI entry
│   ├── cli.rs                    # clap commands
│   ├── config.rs                 # runts.config.json parser
│   ├── lib.rs                    # Library exports
│   ├── transpile/                # Compiler pipeline
│   │   ├── mod.rs                # Transpiler orchestration
│   │   ├── parser.rs             # TS/TSX parser
│   │   ├── hir.rs                # HIR definitions
│   │   ├── analyzer.rs           # Semantic analysis
│   │   ├── codegen.rs            # Rust code generation
│   │   ├── jsx_transformer.rs    # JSX → html! macro
│   │   ├── routegen.rs           # Route table generation
│   │   ├── middlewaregen.rs      # Middleware pipeline gen
│   │   ├── errors.rs             # Error reporting
│   │   └── tests.rs              # Unit tests
│   ├── runtime/                  # HIR interpreter + types
│   │   ├── mod.rs
│   │   ├── interpreter.rs        # HIR executor (dev mode)
│   │   ├── middleware.rs         # Middleware runtime
│   │   ├── component.rs          # Component trait
│   │   ├── vdom.rs               # VNode types
│   │   ├── signals.rs            # Signal runtime
│   │   ├── hooks.rs              # Hook implementations
│   │   ├── islands.rs            # Island runtime
│   │   ├── server.rs             # Server utilities
│   │   ├── prelude.rs            # Re-exports
│   │   └── html.rs               # HTML escaping
│   └── commands/                 # CLI commands
│       ├── mod.rs
│       ├── dev.rs                # Dev server
│       ├── build.rs              # Production build
│       ├── init.rs               # Project scaffolding
│       ├── init_templates/       # Template files
│       ├── add.rs                # Add component/island
│       ├── ssr.rs                # SSR engine
│       ├── routes.rs             # Route handlers
│       ├── layouts.rs            # Layout system
│       └── parallel.rs           # Parallel transpilation
├── crates/
│   ├── runts-lib/                # Runtime library (for generated code)
│   │   └── src/
│   │       ├── lib.rs
│   │       └── runtime/
│   │           ├── mod.rs
│   │           ├── prelude.rs
│   │           ├── component.rs
│   │           ├── vdom.rs
│   │           ├── signals.rs
│   │           ├── hooks.rs
│   │           ├── islands.rs
│   │           └── server.rs
│   ├── runts-macros/             # proc macros
│   │   └── src/
│   │       ├── lib.rs            # #[component] macro
│   │       ├── component.rs      # Component macro impl
│   │       └── html.rs           # html! macro impl
│   └── runts-client/             # Client JS runtime
│       └── src/
│           ├── lib.rs
│           ├── runtime.ts        # Vanilla JS runtime
│           └── hydrate.ts        # Hydration bootstrap
├── docs/
│   ├── ARCHITECTURE.md
│   ├── SUBSET_SPEC.md
│   ├── TRANSPILE_STRATEGY.md
│   ├── ROADMAP.md
│   └── PERFORMANCE.md
├── examples/
│   └── my-blog/                  # Example application
└── tests/
    └── integration/              # Integration tests
```
