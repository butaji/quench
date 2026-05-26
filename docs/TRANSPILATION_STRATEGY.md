# runts — Transpilation & Runtime Strategy

## Architecture Overview

```
TS/TSX Source
      │
      ▼
┌─────────────────┐
│  Parser (swc)   │  ──→  HIR (High-level IR)
│  1830 lines     │       Typed AST representation
└─────────────────┘
      │
      ▼
┌─────────────────┐
│   Analyzer      │  ──→  Semantic validation
│   Type inference│       Island/Route/Component detection
└─────────────────┘
      │
      ▼
┌─────────────────┐
│   CodeGen       │  ──→  Rust source code (in-mem)
│   HIR → Rust    │       Uses html! proc-macro for JSX
└─────────────────┘
      │
      ▼
┌─────────────────┐     ┌─────────────────┐
│  Dev Mode       │     │  Production     │
│  HIR Interpreter│     │  cargo build    │
│  (no compile)   │     │  (native binary)│
│  Axum server    │     │  Axum server    │
└─────────────────┘     └─────────────────┘
```

---

## 1. Parser: TS/TSX → HIR

### 1.1 Why a Custom Parser?

We use a **recursive descent parser** (not swc/tree-sitter) for three reasons:

1. **Determinism:** Full control over error messages and recovery
2. **Subset enforcement:** Reject unsupported constructs at parse time with helpful diagnostics
3. **Zero heavy deps:** No WASM bindings, no JS runtime, no massive grammar files

### 1.2 HIR Design

The HIR is a **Rust-native AST** that captures TypeScript semantics without JS-specific baggage:

```rust
pub enum Expr {
    Ident { name: String },
    String { value: String },
    Number { value: f64 },
    Bool { value: bool },
    Null,
    Undefined,
    Array { elements: Vec<Expr> },
    Object { props: Vec<ObjectProp> },
    JSX(JSXExpr),
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, expr: Box<Expr> },
    Arrow { params: Vec<Param>, body: Box<Expr> },
    Fn { decl: FunctionDecl },
    Await { expr: Box<Expr> },
    Member { obj: Box<Expr>, property: Box<Expr> },
    Index { obj: Box<Expr>, index: Box<Expr> },
    Assign { op: AssignOp, left: Box<Expr>, right: Box<Expr> },
    Ternary { cond: Box<Expr>, yes: Box<Expr>, no: Box<Expr> },
    Template { parts: Vec<TemplatePart> },
    // ...
}
```

Key invariants:
- **Every expression has an inferred type** after analysis
- **JSX is fully parsed into a typed AST**, not string manipulation
- **Async boundaries are explicit** for proper `await` insertion

### 1.3 JSX Parsing

JSX is parsed into a first-class `JSXExpr` node:

```rust
pub struct JSXExpr {
    pub tag: JSXName,           // Element or Component
    pub attrs: Vec<JSXAttr>,   // Attributes + event handlers
    pub children: Vec<JSXChild>,
    pub key: Option<Expr>,     // For list diffing
}
```

Attribute renaming happens at parse time:
- `class` → `class_name`
- `for` → `html_for`
- `onClick` → `on_click`
- `style={{ color: "red" }}` → parsed as `Object` expression

---

## 2. Semantic Analyzer

### 2.1 Type Inference

A unification-based type inferencer resolves types bottom-up:

1. **Literal types:** `"hello"` → `Type::String`, `42` → `Type::Number`
2. **Operator resolution:** `a + b` requires both operands be `Number` or `String`
3. **Function return types:** Inferred from body if not annotated
4. **Generic monomorphization:** `useState<number>(0)` → `use_state::<f64>(0.0)`

### 2.2 Component Detection

Components are detected by **heuristic + annotation**:

```rust
fn is_component(decl: &FunctionDecl) -> bool {
    // PascalCase function name in islands/ or components/ dir
    decl.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
    // OR explicit return type annotation of JSX/VNode
    // OR default export from routes/ file
}
```

Detected components get:
- `#[component]` attribute macro
- Props struct generation (from destructured params)
- VNode return type enforcement

### 2.3 Route Extraction

Routes are extracted from `routes/` directory:

| File Pattern | Route Pattern | Params |
|--------------|---------------|--------|
| `index.tsx` | `/` | — |
| `about.tsx` | `/about` | — |
| `[id].tsx` | `/:id` | `id` |
| `[...slug].tsx` | `/*slug` | `slug` |
| `blog/index.tsx` | `/blog` | — |

Handlers are extracted from `export const handler = { ... }` objects.

### 2.4 Island Detection

Files in `islands/` directory are **automatically islands**:
- PascalCase function exports become island components
- Props must be `Serialize + Deserialize` (enforced at analysis)
- Hydration strategy inferred from props or explicit directive

---

## 3. Code Generation: HIR → Rust

### 3.1 Core Strategy

**Principle:** Generate idiomatic Rust that a human would write. No runtime JS emulation.

### 3.2 Type Mapping

```typescript
// TypeScript                      // Generated Rust
string                           →  String
number                           →  f64
boolean                          →  bool
T | null                         →  Option<T>
T[]                              →  Vec<T>
Record<K, V>                     →  HashMap<K, V>
(a: T) => U                      →  Box<dyn Fn(T) -> U + Send + Sync>
interface Point { x: number }    →  #[derive(Clone, PartialEq, Serialize, Deserialize)]
                                   struct Point { pub x: f64 }
```

### 3.3 JSX Code Generation

JSX is transformed into `html!` macro invocations:

```tsx
// TypeScript
function Header({ title }: { title: string }) {
  return <h1 className="header">{title}</h1>;
}
```

```rust
// Generated Rust
#[component]
pub fn header(_props: HeaderProps) -> VNode {
    let title = _props.title;
    html!(
        <h1 class_name="header">{title}</h1>
    )
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderProps {
    pub title: String,
}
```

### 3.4 Props Struct Generation

For destructured component params, a Props struct is auto-generated:

```tsx
// TypeScript
export default function Home({ data }: PageProps<HomeData>) { ... }
```

```rust
// Generated Rust
#[component]
pub fn home(_props: PageProps<HomeData>) -> VNode {
    let data = _props.data;
    // ...
}
```

**Critical rule:** Type names are **preserved** in PascalCase. Value identifiers are snake_case.

### 3.5 Hook Mapping

```tsx
// TypeScript
const [count, setCount] = useState(0);
useEffect(() => { console.log(count); }, [count]);
```

```rust
// Generated Rust
let (count, set_count) = use_state(0f64);
use_effect(move || { tracing::info!("{}", count); }, &[count]);
```

### 3.6 Signal Mapping

```tsx
// TypeScript (Preact Signals)
const count = signal(0);
const doubled = computed(() => count.value * 2);
```

```rust
// Generated Rust
let count = signal(0i32);
let doubled = Computed::new(|| *count.read() * 2);
```

Signals auto-dereference in JSX: `{count.value}` → `{count}`.

### 3.7 Event Handler Mapping

```tsx
// TypeScript
<button onClick={() => setCount(c => c + 1)}>+</button>
```

```rust
// Generated Rust
html!(
    <button on_click={move || set_count(count + 1)}">"+"</button>
)
```

Event handlers are boxed closures: `Box<dyn Fn(JsValue) + Send + Sync>`.

### 3.8 Route Handler Generation

```typescript
// TypeScript
export const handler = {
  async GET(_req: Request, ctx: HandlerContext) {
    const data = await fetchData();
    return new Response(JSON.stringify(data), {
      headers: { "Content-Type": "application/json" }
    });
  }
};
```

```rust
// Generated Rust (in route module)
pub async fn handler_get(req: Request, ctx: HandlerContext) -> Response {
    let data = fetch_data().await;
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&data).unwrap()))
        .unwrap()
}
```

### 3.9 Module Structure

Each TS/TSX file generates a Rust module:

```rust
// routes/index.tsx → gen/routes/index.rs
//! Generated by runts from routes/index.tsx

use runts_lib::prelude::*;
use serde::{Serialize, Deserialize};

// Props structs
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeData { /* ... */ }

// Handler
pub async fn handler_get(req: Request, ctx: HandlerContext) -> Response { /* ... */ }

// Component
#[component]
pub fn home(_props: PageProps<HomeData>) -> VNode { /* ... */ }
```

---

## 4. Runtime Architecture

### 4.1 Dual Runtime Model

runts has **two execution modes**:

#### Dev Mode: HIR Interpreter

```
Request → Axum Router → Match Route → HIR Interpreter → VNode tree
                                                         ↓
                                                  html! macro render
                                                         ↓
                                                    HTML Response
```

- **No compilation** on file change
- HIR is re-parsed and interpreted directly
- Hot reload: `<1s` (file watcher + HIR reload)
- Islands rendered as server-side placeholders with hydration scripts

#### Production Mode: Native Binary

```
Request → Axum Router → Match Route → Compiled Rust Handler → VNode tree
                                                              ↓
                                                       html! macro render
                                                              ↓
                                                         HTML Response
```

- Full `cargo build --release`
- Zero interpretation overhead
- Same runtime types, compiled to native code

### 4.2 Virtual DOM / Rendering

The VDOM is **server-centric** (no client-side diffing in Rust):

```rust
pub enum VNode {
    Element {
        tag: String,
        attrs: HashMap<String, AttrValue>,
        children: Vec<VNode>,
        events: HashMap<String, EventHandler>,
    },
    Component {
        name: String,
        props: HashMap<String, serde_json::Value>,
        children: Vec<VNode>,
    },
    Text { value: String },
    Fragment(Vec<VNode>),
    Empty,
}
```

Rendering is **single-pass**:
1. Build VNode tree (component evaluation)
2. Walk tree, emit HTML string
3. For islands: emit `<div data-island="Name" data-props="{}">...</div>` + hydration script

### 4.3 Fine-Grained Reactivity (Signals)

Signals are implemented with **parking_lot RwLock** for zero-cost reads:

```rust
pub struct Signal<T: Clone> {
    inner: Arc<RwLock<T>>,
    subscribers: Arc<RwLock<Vec<Box<dyn Fn() + Send + Sync>>>>,
}

impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T { self.inner.read().clone() }
    pub fn set(&self, value: T) {
        *self.inner.write() = value;
        for sub in self.subscribers.read().iter() { sub(); }
    }
}
```

On the server, signals are **evaluated once** at render time. On the client (islands), signals drive DOM updates via the JS runtime.

### 4.4 Islands Architecture

```
SSR Phase (Rust):
  1. Render island component → VNode
  2. Emit HTML with data-island + data-props attributes
  3. Include island JS bundle reference

Hydration Phase (Client JS):
  1. Parse data-props JSON
  2. Reconstruct component state
  3. Attach event listeners
  4. Set up signal subscriptions
  5. Replace static HTML with reactive DOM
```

Hydration strategies:
| Strategy | Trigger |
|----------|---------|
| `load` | `DOMContentLoaded` |
| `idle` | `requestIdleCallback` |
| `visible` | `IntersectionObserver` |
| `interaction` | Click/hover on placeholder |

### 4.5 `html!` Proc Macro

The `html!` macro (in `crates/runts-macros`) parses JSX-like syntax at compile time:

```rust
html! {
    <div class_name="foo" on_click={handler}>
        Hello {name}
        <Counter initial={5} />
    </div>
}
```

Expands to:
```rust
VNode::Element {
    tag: "div".into(),
    attrs: [("class_name", "foo".into())].into(),
    events: [("on_click", Box::new(handler))].into(),
    children: vec![
        VNode::Text { value: "Hello ".into() },
        VNode::Text { value: name.to_string() },
        VNode::Component { name: "Counter".into(), props: [...], children: vec![] },
    ],
}
```

### 4.6 Client JS Runtime

For island hydration, a minimal JS runtime (~2KB gzipped) is injected:

```typescript
// crates/runts-client/src/runtime.ts
interface IslandRuntime {
  register(name: string, factory: (props: any) => void): void;
  hydrate(el: HTMLElement, name: string, props: any): void;
  signal<T>(initial: T): Signal<T>;
  effect(fn: () => void): void;
}
```

This runtime is **hand-written** (no Preact dependency) and provides:
- Signal reactivity via proxy-based observation
- VDOM diffing for island boundaries only
- Event delegation

---

## 5. Server Architecture (Axum/Tower)

### 5.1 Dev Server

```rust
let app = Router::new()
    .nest_service("/static", get(handle_static))
    .route("/_runts/islands/:name", get(handle_island_bundle))
    .route("/_runts/hmr", get(handle_hmr_sse))
    .route("/_runts/client.js", get(client_script))
    .route("/api/*path", get(handle_api))
    .route("/*path", get(handle_ssr))
    .with_state(AppState::new(root)?);
```

### 5.2 Production Server

Generated `main.rs`:

```rust
#[tokio::main]
async fn main() {
    let app = my_project::build_router()
        .layer(TraceLayer::new_for_http());
    
    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

The `build_router()` function is generated from the route manifest:

```rust
pub fn build_router() -> Router {
    Router::new()
        .route("/", get(handlers::index_handler))
        .route("/blog", get(handlers::blog_index_handler))
        .route("/blog/:slug", get(handlers::blog_slug_handler))
        .route("/about", get(handlers::about_handler))
        .layer(middleware::from_fn(middleware::global_middleware))
}
```

---

## 6. Middleware Pipeline

### 6.1 Execution Order

```
Global _middleware.ts
    → Route _middleware.ts
        → Layout _layout.tsx
            → Route Component
```

### 6.2 State Propagation

```rust
pub struct HandlerContext {
    pub request: Request,
    pub state: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    pub params: HashMap<String, String>,
}

impl HandlerContext {
    pub async fn next(&mut self
    ) -> Result<Response, ...> { /* ... */ }
    
    pub fn render<P: Serialize>(
        &self,
        component: fn(P) -> VNode,
        props: P
    ) -> Response { /* ... */ }
}
```

---

## 7. Error Handling Strategy

### 7.1 Compile-Time Errors

The analyzer produces structured errors:

```rust
pub struct TranspileError {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub suggestion: Option<String>,
    pub code: ErrorCode,
}
```

Example output:
```
error[E003]: Unsupported feature
  → routes/index.tsx:42:5
  │
42│   class MyComponent extends Component {
  │   ^^^^^ Class components are not supported
  │
  = help: Convert to a function component:
  │
  │     function MyComponent({ name }: Props) {
  │       return <div>{name}</div>;
  │     }
```

### 7.2 Runtime Errors

Dev mode: Errors render as HTML with stack trace
Production: Errors logged via `tracing`, generic 500 page

### 7.3 Error Pages

Custom `_404.tsx` and `_500.tsx` in `routes/`:

```tsx
export default function Error404() {
  return <div><h1>404 — Page Not Found</h1></div>;
}
```

---

## 8. Incremental & Parallel Compilation

### 8.1 Parallel Transpilation

```rust
use rayon::prelude::*;

files.par_iter()
    .map(|path| transpile_file(path))
    .collect::<Result<Vec<_>>>()
```

### 8.2 Incremental Builds (Planned v0.6)

- File-level content hashing
- Cache `.runts/cache/` directory
- Only re-transpile changed files
- Dependency graph tracking for invalidation

---

## 9. Security Model

| Concern | Mitigation |
|---------|------------|
| XSS | All text content HTML-escaped at render time |
| `dangerouslySetInnerHTML` | Not supported |
| `eval()` / `new Function()` | Rejected at parse time |
| Path traversal | Static file handler validates paths |
| SSRF in `fetch` | Sandbox via `reqwest` timeout limits |
| Prototype pollution | No prototype chain in runtime |

---

## 10. Performance Characteristics

| Phase | Time Target | Implementation |
|-------|-------------|----------------|
| Parse single file | <5ms | Recursive descent, zero deps |
| Type analysis | <10ms | Local inference, no cross-module |
| Code generation | <5ms | String building |
| Dev server cold start | <1s | Parse all files into HIR |
| Dev hot reload | <100ms | Single file re-parse + HIR reload |
| Production build (transpile) | <2s for 100 files | Parallel processing |
| Production build (cargo) | Standard Rust LTO | `lto = true`, `codegen-units = 1` |
| Request latency (SSR) | <1ms | Native Rust, no JS overhead |
| Binary size | <2MB | `panic = "abort"`, `strip = true`, `opt-level = "z"` |
