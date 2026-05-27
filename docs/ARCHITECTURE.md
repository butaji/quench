# runts — Architecture & Transpilation Strategy

> **Version:** 0.5.0  
> **Scope:** Parser → HIR → Semantic Analysis → Rust Codegen → Runtime  
> **Constraint:** Zero external JS runtimes (no V8, no Deno, no WebAssembly JS)

---

## 1. High-Level Architecture

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
│              ▼                                               ▼               │
│  ┌─────────────────────────┐                     ┌─────────────────────────┐│
│  │    Development Mode      │                     │    Production Mode      ││
│  │                          │                     │                          ││
│  │  HIR → Interpreter       │                     │  Rust codegen → cargo    ││
│  │  (direct execution)      │                     │  build --release         ││
│  │                          │                     │  (static binary)         ││
│  │  File watcher + SSE HMR  │                     │  Axum + Tower server     ││
│  │  Target: <50ms reload    │                     │  Target: <2MB binary     ││
│  └─────────────────────────┘                     └─────────────────────────┘│
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Phase 1: Parser (TSX → HIR)

### 2.1 Design Decision: Custom Recursive Descent

runts uses a **custom recursive descent parser** rather than swc/oxc. This is a deliberate trade-off:

- **Pros:** Zero parser dependencies, full control over error messages, easy to restrict to our subset, fast compilation of runts itself.
- **Cons:** TypeScript is a massive language; our parser covers the supported subset only.
- **Future:** v1.0 will migrate to `oxc_parser` for full TypeScript compliance without maintaining a custom parser.

### 2.2 Parsing Pipeline

```
TS/TSX Source
    │
    ▼
┌────────────────────────────────────────┐
│  Tokenizer                             │
│  • Keywords, identifiers, literals     │
│  • JSX tokens (<, />, {, })            │
│  • Template literal fragments          │
└────────────────────────────────────────┘
    │
    ▼
┌────────────────────────────────────────┐
│  Recursive Descent Parser              │
│  • Module items (imports, exports)     │
│  • Statements (var, if, for, return)   │
│  • Expressions (call, member, binary)  │
│  • JSX (elements, components, attrs)   │
│  • Types (annotations, interfaces)     │
└────────────────────────────────────────┘
    │
    ▼
┌────────────────────────────────────────┐
│  HIR Builder                           │
│  • Flatten hoisted vars                │
│  • Normalize destructuring             │
│  • Strip type annotations              │
│  • Tag JSX with component vs HTML      │
└────────────────────────────────────────┘
    │
    ▼
HIR Module
```

### 2.3 HIR (High-Level IR) Structure

The HIR is a simplified, serializable AST that discards TypeScript-specific syntax while preserving all runtime semantics.

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
    Function(FunctionDecl),   // function / async function
    Variable(VariableDecl),   // const / let / var
    Type(TypeDecl),           // interface / type alias (erased at runtime)
    Class(ClassDecl),         // Detected → E010 error
}
```

Key HIR properties:
- **Typed but erasable:** Every expression carries a `Type`, but codegen ignores it except for struct generation.
- **Normalized:** `var` is flattened to `let`. Destructuring is expanded to individual assignments.
- **JSX-native:** JSX is preserved as first-class `JSXExpr` nodes, not desugared to `React.createElement`.

---

## 3. Phase 2: Semantic Analyzer

### 3.1 Responsibilities

The analyzer performs **subset validation** and **framework-specific extraction**:

1. **Subset Enforcement** — Reject excluded features (classes, eval, dynamic imports) with precise error codes.
2. **Hook Validation** — Verify hooks are called at top level, in consistent order, and only inside components.
3. **Module Classification** — Determine if a file is a route, island, layout, middleware, or component.
4. **Route Extraction** — Compute the URL pattern from the file path.
5. **Island Detection** — Mark default exports in `islands/` as interactive components.
6. **Type Harvesting** — Collect interfaces and type aliases for Rust struct generation.

### 3.2 Module Classification Rules

| Path Pattern | Classification | Detection Rule |
|-------------|----------------|----------------|
| `routes/_middleware.ts` | Global middleware | Filename starts with `_middleware` |
| `routes/_layout.tsx` | Root layout | Filename is `_layout.tsx` |
| `routes/blog/_layout.tsx` | Section layout | Filename is `_layout.tsx` in subdir |
| `routes/index.tsx` | Static route | No `[param]` in path |
| `routes/[slug].tsx` | Dynamic route | Contains `[param]` |
| `routes/[...path].tsx` | Catch-all route | Contains `[...param]` |
| `islands/*.tsx` | Island | Inside `islands/` directory |
| `components/*.tsx` | Static component | Inside `components/` directory |

### 3.3 Hook Rule Checker

```rust
pub fn validate_hooks(body: &[Stmt]) -> Result<(), Vec<AnalyzeError>> {
    // Depth-first scan of the function body
    // Any CallExpr to useState/useEffect/etc. must be at depth 0
    // (top-level of the function, not inside if/for/while/arrow)
    // 
    // Violations emit E009 with location
}
```

---

## 4. Phase 3: Rust Code Generation

### 4.1 Generation Strategy

Codegen transforms HIR into **idiomatic, typed Rust source code** that compiles with `rustc` via `cargo`. The output is never hand-edited; it is regenerated on every production build.

### 4.2 TS/TSX → Rust Transform Rules

#### Functions

```typescript
// Input
export default function Home({ data }: PageProps<Data>) {
  return <div>{data.title}</div>;
}
```

```rust
// Output
#[derive(Default, Deserialize)]
pub struct HomeProps { pub data: Data }

pub fn home(props: HomeProps) -> VNode {
    html! {
        <div>{ props.data.title }</div>
    }
}
```

#### Route Handlers

```typescript
// Input
export const handler = {
  GET: async (req, ctx) => {
    const post = await getPost(ctx.params.slug);
    return ctx.render({ post });
  }
};
```

```rust
// Output
pub async fn handle_get(
    ctx: HandlerContext,
    req: Request,
) -> impl IntoResponse {
    let post = get_post(ctx.params.slug.clone()).await;
    let page_data = serde_json::json!({ "post": post });
    // Render wrapper calls home_render_with_data(...)
    home_render_with_data(ctx.params, ctx.url, page_data).into_response()
}
```

#### Islands

Islands generate **two artifacts**:

1. **Server component** — Same as static component, but wrapped with SSR placeholder + hydration manifest.
2. **Client bundle** — Generated by `js_codegen.rs` as vanilla JS using the Runts client runtime.

```rust
// Server-side placeholder (injected during SSR)
div class="__island__"
    data-island="Counter"
    data-id="island-1"
    data-props="{\"initial\":5,\"step\":1}"
    data-strategy="visible"
    {
        // SSR-rendered HTML
        p { "Count: 5" }
        button { "+" }
    }
```

#### JSX → `html!` Macro

JSX is transformed into invocations of the `html!` procedural macro (from `runts-macros`):

```typescript
// Input
<div class="container">
  <h1>{title}</h1>
  {items.map(item => <Item key={item.id} name={item.name} />)}
</div>
```

```rust
// Output
html! {
    <div class="container">
        <h1>{ title }</h1>
        {
            items.iter().map(|item| {
                html! { <Item key={item.id.clone()} name={item.name.clone()} /> }
            }).collect::<Vec<_>>()
        }
    </div>
}
```

### 4.3 Type Mapping in Codegen

The codegen uses the HIR type information to emit strongly-typed Rust:

```typescript
interface User {
  id: string;
  age: number;
  active: boolean;
  tags: string[];
  profile: { bio: string } | null;
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub age: f64,
    pub active: bool,
    pub tags: Vec<String>,
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub bio: String,
}
```

---

## 5. Runtime Architecture

### 5.1 Server Runtime Stack (Production)

```
┌─────────────────────────────────────────────────────────────────┐
│                     Axum HTTP Server                             │
│  • Router from generated routes.rs                               │
│  • Tower middleware (CORS, compression, trace)                  │
│  • Static file serving (tower-http)                             │
├─────────────────────────────────────────────────────────────────┤
│                     SSR Renderer                                 │
│  • Component tree → VNode tree                                   │
│  • VNode → HTML string (synchronous, zero allocation)           │
│  • Island placeholder injection                                  │
│  • Layout composition (nested _layout.tsx)                      │
├─────────────────────────────────────────────────────────────────┤
│                     VDOM / Template Engine                       │
│  • VNode: Element | Component | Text | Fragment | Empty         │
│  • Render trait: render_to_html()                               │
│  • HTML escaping (automatic)                                    │
├─────────────────────────────────────────────────────────────────┤
│                     Reactivity (Signals)                         │
│  • Signal<T>: fine-grained reactive container                   │
│  • Computed<T>: auto-derived signal                             │
│  • Effect: side-effect subscription                             │
│  • batch() + untrack()                                           │
├─────────────────────────────────────────────────────────────────┤
│                     Hooks Engine                                 │
│  • Thread-local indexed storage (React semantics)               │
│  • use_state, use_effect, use_ref, use_memo, use_callback       │
│  • use_reducer, use_context, use_id                             │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 Signal System (Fine-Grained Reactivity)

runts uses a **Leptos-inspired signal system** rather than a full VDOM diffing algorithm:

```rust
pub struct Signal<T: Clone> {
    value: Arc<RwLock<T>>,
    subscribers: Arc<RwLock<HashSet<usize>>>,
}

impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T {
        if let Some(effect_id) = current_effect_id() {
            self.subscribers.write().insert(effect_id);
        }
        self.value.read().clone()
    }

    pub fn set(&self, value: T) {
        *self.value.write() = value;
        self.notify(); // schedules effects
    }
}
```

**Why signals over VDOM diffing?**
- **Performance:** Updates touch only the affected DOM nodes; no tree diffing.
- **Memory:** No virtual DOM tree retained on the server; SSR is pure string building.
- **Predictability:** Effects run exactly when their dependencies change.

On the server, signals are used primarily for:
- Hook state storage
- Middleware state propagation
- Server-side computed values

On the client (in islands), signals drive DOM updates directly.

### 5.3 Islands Architecture & Partial Hydration

```
Server SSR:
┌─────────────────────────────────────────────────────────────┐
│ <html>                                                       │
│   <body>                                                     │
│     <nav>...</nav>                ← Static HTML (no JS)     │
│     <main>                                                    │
│       <div data-island="Counter"                              │
│            data-id="i-1"                                      │
│            data-props="{...}"                                 │
│            data-strategy="visible">                           │
│         <p>Count: 5</p>           ← SSR placeholder          │
│       </div>                                                  │
│     </main>                                                   │
│     <script>window.__ISLAND_MANIFEST__=[...]</script>        │
│     <script src="/_runts/client.js"></script>               │
│   </body>                                                     │
│ </html>                                                       │
└─────────────────────────────────────────────────────────────┘

Client Hydration:
1. Parse __ISLAND_MANIFEST__
2. Group islands by hydration strategy
3. Eager: hydrate immediately
4. Visible: IntersectionObserver → hydrate when in viewport
5. Idle: requestIdleCallback → hydrate when browser idle
6. Manual: expose Runts.hydrateIsland(id) for explicit trigger
```

**Partial Hydration Guarantees:**
- Static content is **never** hydrated. No JS bundle is loaded for it.
- Each island is an independent hydration unit.
- Islands can be lazy-loaded; the manifest references them by name.
- SSR HTML inside island placeholders is replaced atomically on hydration.

### 5.4 Client Runtime (Browser)

The client runtime is a **zero-dependency vanilla JS file** (`crates/runts-client/src/runtime.ts`) that provides:

1. **Signal system** — Preact Signals-compatible API
2. **Effect system** — Auto-subscription, cleanup, batching
3. **VNode renderer** — Simple DOM diffing for island updates
4. **Hydration bootstrap** — Discovers and hydrates islands

It is **not** a full React/Preact implementation. It only needs to support the patterns used in islands (the supported subset).

Size budget: **< 5KB gzipped**.

---

## 6. Development Mode: HIR Interpreter

### 6.1 Philosophy: Zero Compilation

In development mode, runts **does not compile to Rust**. It parses TS/TSX to HIR and executes the HIR directly via an interpreter. This enables:

- **< 50ms hot reload** (file change → visible update)
- **No cargo build cycles** during UI iteration
- **Identical semantics** to production (same parser, same HIR)

### 6.2 Interpreter Architecture

```
Request
  │
  ▼
┌─────────────────────────────────────────┐
│  Route Matcher (file-based)             │
│  • Regex-based pattern matching          │
│  • Param extraction                      │
└─────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────┐
│  Middleware Pipeline (interpreter)      │
│  • Execute _middleware.ts bodies         │
│  • ctx.next() chaining                   │
│  • State accumulation                    │
└─────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────┐
│  Route Handler (interpreter)            │
│  • Execute handler.GET/POST body         │
│  • Support ctx.render(data)              │
│  • Support new Response()                │
└─────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────┐
│  Component Renderer (interpreter)       │
│  • Execute default export function       │
│  • Hook state (thread-local indexed)     │
│  • JSX → HTML string                     │
│  • Island placeholder injection          │
└─────────────────────────────────────────┘
  │
  ▼
┌─────────────────────────────────────────┐
│  Layout Composition                     │
│  • Nest layouts from root to leaf        │
│  • _app.tsx wrapper                      │
└─────────────────────────────────────────┘
  │
  ▼
HTML Response
```

### 6.3 Expression Evaluation

The interpreter supports all expressions in the supported subset:

```rust
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
    Function(String), // Named reference
}
```

Evaluation is **tree-walk interpretation** — simple, correct, and fast enough for dev mode. No JIT, no bytecode.

### 6.4 Hot Reload Mechanism

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

**Why full page refresh instead of HMR?**
- **Correctness:** Interpreter state is fully reset; no stale closures or hooks.
- **Simplicity:** No need for module hot-swapping or React Fast Refresh equivalent.
- **Speed:** < 50ms end-to-end means full refresh is acceptable.

Future v0.8+ may implement fine-grained HMR for CSS and island props.

---

## 7. Production Mode: Native Compilation

### 7.1 Build Pipeline

```bash
runts build --release
```

```
Phase 1: Transpile (runts)
────────────────────────────
For each TS/TSX file:
  1. Parse → HIR
  2. Analyze → validate subset
  3. Generate → Rust source in src/gen/
  4. Write mod.rs files

Generate:
  • src/routes.rs     (Axum router)
  • src/islands.rs    (Island manifest)
  • src/components.rs (Component re-exports)
  • src/lib.rs        (Library root)
  • src/main.rs       (Entry point, if missing)

Phase 2: Compile (cargo)
────────────────────────
cargo build --release
  • LTO = true
  • codegen-units = 1
  • panic = abort
  • strip = true

Output: target/release/<app>  (static binary)
```

### 7.2 Generated Rust Structure

```
src/
├── main.rs              # Entry point: axum::serve
├── lib.rs               # Re-exports + component registration
├── routes.rs            # Axum router construction
├── islands.rs           # Island metadata + re-exports
├── components.rs        # Component re-exports
└── gen/
    ├── mod.rs
    ├── index.rs         # routes/index.tsx → Rust
    ├── about.rs
    ├── blog/
    │   ├── mod.rs
    │   ├── index.rs
    │   ├── slug.rs      # [slug].tsx
    │   └── _layout.rs
    ├── islands/
    │   ├── mod.rs
    │   ├── counter.rs
    │   └── todo_list.rs
    └── components/
        ├── mod.rs
        └── header.rs
```

### 7.3 Binary Size Optimization

| Technique | Impact |
|-----------|--------|
| LTO (Link Time Optimization) | Removes unused functions across crates |
| Single codegen unit | Maximum optimization, slower compile |
| `panic = abort` | No unwinding tables |
| `strip = true` | Remove debug symbols |
| Static linking | No dynamic library dependencies |
| Compression (optional) | `upx` or `gz` for distribution |

Target: **< 2MB** for a complete app with routing, SSR, and islands.

---

## 8. Dev vs Production Comparison

| Aspect | Development | Production |
|--------|-------------|------------|
| **Parser** | Custom recursive descent | Same |
| **Execution** | HIR interpreter | Native Rust binary |
| **Server** | Axum (interpreter backend) | Axum (generated handlers) |
| **SSR** | Interpreter tree-walk | Native VNode rendering |
| **Hot Reload** | < 50ms (HIR cache invalidation) | N/A (static binary) |
| **Hook State** | Thread-local indexed Vec | Same |
| **Signals** | Rust signals (interpreter) | Same |
| **Binary Size** | N/A (runts CLI) | < 2MB |
| **Throughput** | ~1k req/s (interpreted) | > 50k req/s (native) |
| **Cold Start** | N/A | < 5ms |
| **JS Runtime** | None | None |

---

## 9. Alternative / More Efficient Solutions

Where the standard approach has trade-offs, runts provides alternatives:

### 9.1 JSX Rendering: VDOM vs Template Literals

- **Default:** VDOM with `VNode` tree → HTML string. Flexible, component-centric.
- **Alternative:** For pages with no islands, codegen can emit raw `format!` strings instead of VNode trees. This skips all allocation and renders directly to `String`. Activated via `// @static` comment or `components/` directory.

### 9.2 Reactivity: Signals vs VDOM Diffing

- **Default:** Fine-grained signals (Leptos-style) for islands.
- **Alternative:** For simple islands, the client runtime can use a raw DOM manipulation approach without signals. Activated when an island has no `useState` / `useSignal` hooks.

### 9.3 Hydration: Full vs Partial

- **Default:** Partial hydration per island.
- **Alternative:** `data-strategy="static"` renders the island server-side and never ships JS for it. Useful for "islands" that only need SSR props but no interactivity.

### 9.4 Routing: File-Based vs Config-Based

- **Default:** File-based routing (`routes/` directory).
- **Alternative:** Manual route table in `runts.config.json` for dynamic routes that don't map to files.

---

## 10. Crate Structure

```
runts/                          # CLI + compiler
├── src/
│   ├── main.rs                 # CLI entry
│   ├── cli.rs                  # Clap argument parsing
│   ├── config.rs               # runts.config.json schema
│   ├── transpile/              # Compiler pipeline
│   │   ├── parser.rs           # TSX → HIR
│   │   ├── hir.rs              # HIR definitions
│   │   ├── analyzer.rs         # Semantic analysis
│   │   ├── codegen.rs          # HIR → Rust
│   │   ├── jsx_transformer.rs  # JSX → html! macro
│   │   ├── js_codegen.rs       # HIR → JS (island bundles)
│   │   ├── routegen.rs         # Route table generation
│   │   └── errors.rs           # Error codes + messages
│   ├── runtime/                # Dev + shared runtime
│   │   ├── interpreter.rs      # HIR interpreter
│   │   ├── vdom.rs             # VNode + Render
│   │   ├── signals.rs          # Signal + Computed + Effect
│   │   ├── hooks.rs            # useState, useEffect, etc.
│   │   ├── component.rs        # Component registry
│   │   ├── islands.rs          # Island registry + SSR placeholders
│   │   ├── middleware.rs       # Middleware pipeline executor
│   │   └── server.rs           # SSR engine + static file serving
│   └── commands/               # CLI subcommands
│       ├── dev.rs              # Dev server (HIR interpreter)
│       ├── build.rs            # Production build
│       ├── init.rs             # Project scaffolding
│       └── add.rs              # Component/route generator
│
crates/
├── runts-lib/                  # Runtime library (used by generated code)
│   └── src/runtime/
│       ├── prelude.rs          # Re-exports for generated modules
│       └── ...                 # (mirrors src/runtime/)
├── runts-macros/               # Procedural macros
│   └── src/
│       ├── html.rs             # html! macro implementation
│       └── component.rs        # #[component] macro
├── runts-client/               # Browser runtime
│   └── src/runtime.ts          # Vanilla JS signals + hydration
│
examples/
└── my-blog/                    # Working example project
```

---

*Last updated: 2026-05-27*
