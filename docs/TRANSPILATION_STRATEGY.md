# runts Transpilation & Runtime Strategy

> **⚠️ STALE DOCUMENT:** This doc describes the pre-rquickjs architecture (HIR interpreter, custom parser, Axum dev server). The current dev engine is **rquickjs** with **Yoga** layout. Update in progress — see `tasks/031-update-docs.md`.
>
> **Version:** 0.5.0  
> **Core principle:** TS/TSX → HIR → In-memory Rust codegen → Native binary. Zero JS runtime.

---

## 1. Pipeline Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         runts Transpilation Pipeline                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  TS/TSX Source                    HIR (High-Level IR)         Rust Source   │
│  ─────────────                    ───────────────────         ───────────   │
│  routes/index.tsx                 Module {                     src/gen/...  │
│  islands/Counter.tsx    ──▶       items: [                     islands/...  │
│  components/Header.tsx            Decl(Function(...)),          components/..│
│  middleware/_middleware.ts        Export(Default(...)),        lib.rs       │
│                                   Import(...),                 main.rs      │
│                                 ]                           ─────────────   │
│                                 }                           cargo build ──▶ │
│                                                             Native Binary   │
│                                                                              │
│  Dev Mode Shortcut:                                                          │
│  TS/TSX ──▶ HIR ──▶ Interpreter ──▶ HTML (no Rust compilation)              │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 1.1 Why HIR?

We use an intermediate representation instead of direct TS→Rust string manipulation because:

1. **Semantic analysis** needs a typed tree to detect islands, hooks, route patterns
2. **Dev mode** transpiles TSX → JS bundle → rquickjs eval. Compile path uses HIR → Rust codegen. Same parser, different backends.
3. **Future optimizations** (dead code elimination, constant folding) operate on HIR
4. **Error messages** can reference source positions through the HIR

---

## 2. Phase 1: Parsing (TS/TSX → HIR)

### 2.1 Parser Design

- **Recursive descent**, hand-written, zero external dependencies
- No SWC/Babel/TypeScript compiler — we parse only the supported subset
- ~2,000 LOC, single-pass with minimal backtracking
- JSX is parsed as expression grammar (treats `<` as JSX start when contextually valid)

### 2.2 HIR Structure

```rust
pub struct Module {
    pub source: String,
    pub items: Vec<ModuleItem>,
    pub types: TypeMap,
}

pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
    Class(ClassDecl), // Error in analyzer
}

pub enum Expr {
    JSX(JSXExpr),
    Call { callee: Box<Expr>, args: Vec<Expr>, type_args: Vec<Type> },
    Arrow { params: Vec<Param>, body: Box<Stmt>, is_async: bool },
    Function { decl: FunctionDecl },
    Member { object: Box<Expr>, property: Box<Expr>, computed: bool, optional: bool },
    // ... 30+ variants
}
```

### 2.3 Type Representation

Types are preserved in HIR for codegen decisions:

```rust
pub enum Type {
    String, Number, Boolean, Null, Undefined, Void, Never,
    Unknown, Any, BigInt, Symbol,
    Ref { name: String, generics: Vec<Type> },
    Union { types: Vec<Type> },
    Intersection { types: Vec<Type> },
    Array { elem: Box<Type> },
    Tuple { types: Vec<Type> },
    Object { members: Vec<ObjectMember> },
    Function { params: Vec<Type>, ret: Box<Type> },
    // ...
}
```

This enables:
- Union-to-Option conversion (`string | null` → `Option<String>`)
- Interface-to-struct generation
- Generic monomorphization hints

---

## 3. Phase 2: Semantic Analysis

### 3.1 Analyzer Passes

The `Analyzer` performs multiple passes over the HIR:

| Pass | Purpose |
|------|---------|
| **File Classification** | Detect island/route/layout/middleware from file path |
| **Import Analysis** | Track hooks, signals, Fresh types from imports |
| **Hook Validation** | Ensure hooks are called at top level, in consistent order |
| **Component Detection** | Identify functions starting with uppercase as components |
| **Route Extraction** | Derive Axum route patterns from file paths |
| **Error Collection** | Report unsupported features (classes, dynamic import, etc.) |

### 3.2 Fresh Pattern Detection

```rust
// From file path:
routes/blog/[slug].tsx        → route_pattern = "/blog/:slug"
islands/Counter.tsx           → is_island = true
routes/_layout.tsx            → is_layout = true
routes/_middleware.ts         → is_middleware = true

// From AST:
export const handler = { GET: ... }  → HttpMethod::GET
export default function Page()      → default_component_name = "Page"
```

---

## 4. Phase 3: Rust Code Generation

### 4.1 Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Struct-based props** | Type-safe, zero-cost, serde-compatible |
| **`html!` macro for JSX** | Compile-time HTML validation, near-zero overhead |
| **`#[component]` attribute** | Marks functions for dev tools and debug info |
| **Axum for routing** | Tower ecosystem, middleware composability, production-hardened |
| **`parking_lot` locks** | Faster than std, no poisoning |

### 4.2 Expression Mapping

```typescript
// TypeScript                        // Generated Rust
const x = 5;                    →   let x = 5.0;
const y = "hello";              →   let y = "hello".to_string();
const z = a + b;                →   let z = (a + b);
const arr = [1, 2, 3];          →   let arr = vec![1.0, 2.0, 3.0];
const obj = { a: 1 };           →   let obj = HashMap::from([("a", 1.0)]);

// JSX
return <div class="x">{children}</div>;
→
return html! {
    <div class="x">{ children }</div>
};

// Hooks
const [count, setCount] = useState(0);
→
let (count, set_count) = use_state(0.0);

// Array methods
items.map(x => <Item key={x.id} /&gt;)
→
items.iter().map(|x| {
    html! { <Item key={x.id} /> }
}).collect::<Vec<_>>()

// Destructuring
const { slug } = ctx.params;
→
let _props = ctx.params;
let slug = _props.slug;
```

### 4.3 Route Handler Generation

Fresh handler objects become Axum handler functions:

```typescript
// Source
export const handler = {
    GET: async (req: Request, ctx: HandlerContext) => {
        const post = await getPost(ctx.params.slug);
        return ctx.render({ post });
    },
    POST: async (req: Request, ctx: HandlerContext) => {
        const body = await req.json();
        return new Response(JSON.stringify(body), { status: 201 });
    }
};
```

```rust
// Generated
pub async fn handle_get(
    req: Request,
    ctx: HandlerContext
) -> impl IntoResponse {
    let post = get_post(ctx.params.slug).await;
    ctx.render(PageProps { post })
}

pub async fn handle_post(
    req: Request,
    ctx: HandlerContext
) -> impl IntoResponse {
    let body: serde_json::Value = req.json().await.unwrap();
    let mut resp = Response::builder();
    resp = resp.status(201u16);
    resp.body(Body::from(serde_json::to_string(&body).unwrap())).unwrap()
}
```

### 4.4 Component Generation

Components always return `VNode`:

```rust
#[component]
pub fn counter(props: CounterProps) -> VNode {
    let (count, set_count) = use_state(props.initial);
    html! {
        <div class="counter">
            <button on_click={move |_| set_count(count + 1.0)}>
                "Count: " { count }
            </button>
        </div>
    }
}
```

---

## 5. Runtime Architecture

### 5.1 Development Mode: rquickjs (HIR Interpreter REMOVED)

```
File change ──▶ Parse to HIR ──▶ Update module registry ──▶ Re-render
          (<10ms)              (<5ms)                      (<30ms)
```

- **No Rust compilation** in dev mode
- **Instant hot reload** (<50ms total)
- Interpreter executes HIR `Expr`/`Stmt` directly against a `Value` enum
- JSX is evaluated to HTML strings via `VNode::to_html_string()`
- Islands produce hydration markers (`<div data-island="...">`)

#### Interpreter Value Model

```rust
pub enum Value {
    Undefined, Null, Bool(bool), Number(f64), String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
    Function(String), // Named function reference
}
```

### 5.2 Production Mode: Native Binary

```
HIR ──▶ Code Generator ──▶ Rust Source ──▶ cargo build ──▶ Binary
              │                │                │
              ▼                ▼                ▼
         Type-aware       Idiomatic        LTO + strip
         codegen          Rust code        (~1.5MB binary)
```

Production runtime components:

| Component | Technology | Role |
|-----------|-----------|------|
| HTTP Server | Axum + Tower | Request handling, middleware chain |
| Router | Axum `Router` | Static + dynamic route dispatch |
| SSR Engine | `VNode::render_to_html()` | Synchronous HTML generation |
| Signals | Custom (RwLock + subscriber lists) | Fine-grained island reactivity |
| Serialization | `serde` + `serde_json` | Props serialization |

### 5.3 Islands Architecture

**Server-side:**
1. Detect island component in JSX tree
2. Server-render island to HTML (same component runs on server)
3. Serialize props to JSON
4. Emit hydration marker:
   ```html
   <div data-island="Counter" data-id="island-abc123"
        data-props='{"initial":5}' data-hydrate="visible">
     <button>Count: 5</button>
   </div>
   ```

**Client-side:**
1. Load island manifest from `window.__ISLAND_MANIFEST__`
2. Fetch island JS bundle (or use inline script)
3. Instantiate island class with deserialized props
4. Attach event listeners, wire signals to DOM

**Hydration Strategies:**

| Strategy | Trigger | Use Case |
|----------|---------|----------|
| `load` | `DOMContentLoaded` | Critical UI (navigation, forms) |
| `visible` | IntersectionObserver | Below-fold content |
| `idle` | `requestIdleCallback` | Non-essential widgets |
| `interaction` | Click/hover/scroll | Defer until needed |

---

## 6. Client-Side Island Code Generation

Islands require client-side JavaScript for interactivity. runts generates vanilla JS from the same HIR used for Rust codegen:

```
Island TSX ──▶ Parser ──▶ HIR ──▶ JS Code Generator ──▶ Vanilla JS Bundle
                                      │
                                      ▼
                              ┌───────────────────┐
                              │  useState → Signal│
                              │  JSX → VNode obj  │
                              │  Events → handlers│
                              └───────────────────┘
```

### 6.1 JS Generator Design

- **No external JS bundler**: HIR is transformed directly to vanilla JS using string codegen
- **Zero dependencies**: Generated bundles include only the island logic + a minimal signal runtime
- **Automatic hook shimming**: `useState` → `Runts.signal()`, `useEffect` → `Runts.effect()`
- **JSX → VNode objects**: `{ type: 'div', props: { children: 'hello' } }`

### 6.2 Bundle Structure

```javascript
// 1. Embedded Runts client runtime (signals, hydration, VNode renderer)
// 2. Hook shims (useState, useRef, useMemo, etc.)
// 3. Generated component function from HIR
function CounterComponent(props) {
  const { initial, step, label } = props || {};
  const [count, setCount] = useState(initial);
  const increment = () => { setCount(count.value + step); };
  return { type: 'div', props: { children: [...] } };
}
// 4. Registration
Runts.registerIsland('Counter', CounterComponent);
```

## 7. Fine-Grained Reactivity (Signals)

Instead of VDOM diffing, islands use a Leptos-inspired signal system:

```rust
// Signal with subscriber tracking
pub struct Signal<T: Clone> {
    value: Arc<RwLock<T>>,
    subscribers: Arc<RwLock<Vec<Box<dyn Fn()>>>>,
}

impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T {
        // In effect context: register subscriber
        if let Some(effect) = current_effect() {
            self.subscribers.write().push(effect.clone());
        }
        self.value.read().clone()
    }

    pub fn set(&self, value: T) {
        *self.value.write() = value;
        for subscriber in self.subscribers.read().iter() {
            subscriber();
        }
    }
}
```

**Effects** automatically subscribe to signals accessed during their execution:

```rust
let count = Signal::new(0);
Effect::new({
    let count = count.clone();
    move || {
        println!("Count changed: {}", count.get()); // auto-subscribes
    }
});

count.set(5); // Effect re-runs automatically
```

**Trade-off:** Signals require explicit cleanup on unmount. In practice, islands are long-lived components, so this is acceptable.

---

## 7. Alternative / More Efficient Solutions

### 7.1 StringBuilder SSR vs VDOM

For **pure SSR** (no client-side reactivity), we use direct `String` concatenation instead of VNode trees:

```rust
// Faster for one-shot SSR:
fn render_to_string(&self) -> String {
    let mut s = String::with_capacity(256);
    s.push_str("<div class=\"");
    s.push_str(&self.class);
    s.push_str("\">");
    for child in &self.children { s.push_str(child); }
    s.push_str("</div>");
    s
}
```

**VDOM is only constructed for islands** that need client-side updates.

### 7.2 Compile-Time HTML Validation

The `html!` procedural macro (in `crates/runts-macros`) validates tag names and attribute syntax at compile time:

```rust
html! { <div clas="typo"> } // Compile error: unknown attribute "clas"
```

This catches bugs that would be runtime errors in JS frameworks.

### 7.3 Zero-Copy Props Deserialization

For production, props can be deserialized via `serde` with `#[serde(deny_unknown_fields)]` to catch type mismatches between server and client.

---

## 8. Build Pipeline Detail

### 8.1 Parallel Transpilation

```rust
// Rayon parallel iterator over all TS/TSX files
let results: Vec<_> = files
    .par_iter()
    .map(|path| transpile_single(path))
    .collect();
```

- Route files, island files, and component files are processed in parallel
- Each thread has its own `Parser` + `Analyzer` + `CodeGenerator` instance
- Generated files are collected and deduplicated before writing

### 8.2 Cargo Integration

```rust
pub fn compile_rust(project_root: &Path, release: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(project_root);
    if release { cmd.arg("build").arg("--release"); }
    else { cmd.arg("build"); }
    let output = cmd.output()?;
    // ... error handling
}
```

**Binary size optimizations:**
- `lto = "fat"` in Cargo.toml
- `strip = true`
- `opt-level = "z"` (optional, size vs speed trade-off)

---

## 9. Dev/Production Parity Guarantees

| Feature | Dev (Interpreter) | Production (Native) | Parity |
|---------|-------------------|---------------------|--------|
| JSX rendering | HIR → HTML strings | `html!` macro → HTML | ✅ Identical output |
| Route matching | Regex table | Axum router | ✅ Same patterns |
| Middleware | Sequential HIR exec | Tower layers | ✅ Semantics match |
| Hooks | Simulated (no re-render) | Full signal graph | ⚠️ Dev doesn't re-render; production does |
| Islands | SSR markers + static JS | SSR markers + bundled JS | ✅ Same markers |

**Known divergence:** Dev mode does not re-render components on state changes (no signal graph in interpreter). This is acceptable because dev mode is for content/layout iteration; interactivity is tested in production builds or via browser DevTools.

