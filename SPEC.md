# runts Specification

**Version**: 0.1.0  
**Status**: MVP Implementation  
**Last Updated**: 2024

---

## Overview

runts is a CLI tool that provides full framework-level compatibility with [Fresh](https://fresh.deno.dev/docs/concepts/architecture) and Preact using a well-defined efficient subset of TypeScript + TSX, compiling directly to native Rust binaries.

### Core Principles

1. **Zero External JS Runtimes**: No V8, Deno, or WebAssembly JS engines
2. **Pure Native Compilation**: TS/TSX → HIR → Rust source → binary
3. **Fresh Compatibility**: Users write Fresh-style Preact TSX with minimal changes
4. **Dual-Mode Operation**: Instant interpretation (dev) vs. compiled binaries (prod)

---

## Part 1: Supported TS/TSX Subset

### 1.1 Language Features

#### ✅ Fully Supported

| Feature | Syntax | Notes |
|---------|--------|-------|
| **Type Annotations** | `let x: number = 5` | Basic types only |
| **Interfaces** | `interface Props { name: string }` | Simple interfaces |
| **Type Aliases** | `type Handler = () => void` | Simple aliases |
| **Generics** | `function foo<T>(x: T): T` | Single letter, function-level |
| **Enums** | `enum Color { Red, Blue }` | Numeric enums only |
| **Const Assertions** | `as const` | Object literals |
| **Union Types** | `string \| null` | Two-type unions mapped to Option |
| **Intersection Types** | `A & B` | Simple merges |
| **Template Literals** | `` `hello ${name}` `` | Basic interpolation |

#### ⚠️ Partially Supported (Polyfilled)

| Feature | Status | Fallback |
|---------|--------|----------|
| `namespace` | Warning | Use modules |
| `declare` | Ignored | Types stripped |
| `abstract` | Warning | Treated as regular |
| `readonly` | Ignored | No runtime effect |

#### ❌ Explicitly Excluded

| Feature | Reason | Workaround |
|---------|--------|------------|
| `class` | No class runtime | Use functions + closures |
| `interface extends` | Complex inheritance | Type composition |
| `namespace` | Module conflicts | Use ES modules |
| `declare global` | Type-only | Remove |
| `enum` (string) | Complex runtime | Use `const` objects |
| `private`/`protected` | No OOP | Use closures |
| `namespace` blocks | Deprecated | ES modules |
| `module` declarations | Type-only | Remove |
| Decorators | Complex metadata | Not planned |
| `infer` keyword | Complex inference | Manual types |
| Conditional types | Complex inference | Flatten |
| Mapped types | Complex transformation | Explicit types |
| Template literal types | Rarely used | String concatenation |

### 1.2 Expressions

#### ✅ Fully Supported

```typescript
// Binary operators
a + b, a - b, a * b, a / b, a % b
a === b, a !== b, a == b, a != b
a < b, a <= b, a > b, a >= b
a && b, a || b, a ?? b

// Unary operators
-a, +a, !a, typeof a, void a

// Ternary
a ? b : c

// Template literals
`Hello ${name}!`

// Arrow functions
(a, b) => a + b
(a: number): number => a * 2
() => { return 42; }

// Spread operator
[...arr, ...brr]
{ ...obj, extra: true }

// Destructuring
const { name, age } = obj;
const [first, ...rest] = arr;
const { x: alias } = obj; // rename
```

#### ⚠️ Limited Support

| Expression | Limitation |
|------------|------------|
| `eval()` | Blocked for security |
| `new Function()` | Blocked |
| `with` statement | Blocked |
| Regex literals | Basic only |

### 1.3 JSX/TSX Support

#### ✅ Supported JSX Patterns

```tsx
// HTML elements (lowercase)
<div className="container">
  <h1>Hello</h1>
  <p>{message}</p>
</div>

// Self-closing
<br />
<img src={url} alt="desc" />

// Fragments
<>
  <ChildA />
  <ChildB />
</>

// Conditional rendering
{show && <Component />}
{error ? <Error /> : <Content />}

// List rendering
{items.map(item => (
  <li key={item.id}>{item.name}</li>
))}

// Spread props
<div {...props} />
```

#### ❌ JSX Exclusions

| Pattern | Reason |
|---------|--------|
| `dangerouslySetInnerHTML` | XSS risk |
| Ref forwarding | No class refs |
| Render props pattern | Use composition |
| Portal | Not implemented |
| Suspense boundaries | Not implemented |

### 1.4 Statements

#### ✅ Fully Supported

```typescript
// Variable declarations
const x = 1;
let y = 2;
let z: number;
const { a, b } = obj;

// Functions
function greet(name: string): string {
  return `Hello ${name}`;
}

const add = (a: number, b: number): number => a + b;

// Async functions
async function fetchData(): Promise<Data> {
  const res = await fetch(url);
  return res.json();
}

// Control flow
if (condition) { ... } else { ... }
while (condition) { ... }
for (let i = 0; i < n; i++) { ... }
for (const item of items) { ... }
switch (value) { case x: ... break; }
try { ... } catch (e) { ... }
return value;
throw new Error("msg");
```

#### ❌ Statement Exclusions

| Statement | Reason |
|-----------|--------|
| `do-while` | Rare, complexity |
| `labeled statements` | Rare |
| `with` | Scope ambiguity |
| `debugger` | Dev-only |

### 1.5 Fresh/Preact Specific

#### ✅ Fresh-Style Routes

```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server.ts";

interface PostData {
  title: string;
  content: string;
}

export const handler = {
  GET: async (req: Request, ctx: HandlerContext) => {
    const slug = ctx.params.slug;
    const post = await getPost(slug);
    return ctx.render({ post });
  }
};

export default function BlogPost({ data }: PageProps<PostData>) {
  return (
    <article>
      <h1>{data.post.title}</h1>
      <div>{data.post.content}</div>
    </article>
  );
}
```

#### ✅ Fresh-Style Islands

```typescript
// islands/Counter.tsx
import { useState } from "preact/hooks";

interface CounterProps {
  initial?: number;
  step?: number;
}

export default function Counter({ initial = 0, step = 1 }: CounterProps) {
  const [count, setCount] = useState(initial);
  
  return (
    <div class="counter">
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + step)}>+</button>
      <button onClick={() => setCount(count - step)}>-</button>
    </div>
  );
}
```

#### ✅ Supported Hooks

| Hook | Status | Notes |
|------|--------|-------|
| `useState` | ✅ Full | Generic state |
| `useEffect` | ✅ Full | Cleanup supported |
| `useRef` | ✅ Full | Object.current |
| `useMemo` | ✅ Full | Memoization |
| `useCallback` | ✅ Full | Function memo |
| `useContext` | ⚠️ Basic | Limited scope |
| `useReducer` | ✅ Full | Action dispatch |
| `useLayoutEffect` | ⚠️ Warning | Sync warning |
| `useImperativeHandle` | ❌ No | No refs |
| `useDebugValue` | Ignored | Dev-only |

#### ⚠️ Preact Signals (Partial)

```typescript
// Basic signal support
import { signal, computed } from "@preact/signals";

// Supported
const count = signal(0);
const doubled = computed(() => count.value * 2);

// Not supported
signal Effect, signal batch (use discrete updates)
```

### 1.6 Imports/Exports

#### ✅ Supported Patterns

```typescript
// Named imports
import { useState, useEffect } from "preact/hooks";
import { signal } from "@preact/signals";

// Default imports
import React from "preact";
import Counter from "./islands/Counter";

// Side-effect imports (ignored)
import "./styles.css";

// Re-exports
export { useState, useEffect };
export type { Props };
export default function Component() { ... }

// Dynamic imports
const module = await import("./module.ts");
```

#### ❌ Import Exclusions

| Pattern | Reason |
|---------|--------|
| `import type` | Type-only, stripped |
| `export type` | Type-only |
| `import.meta.url` | Dev-only |
| `import()` (dynamic) | Complex bundling |

---

## Part 2: Transpilation Architecture

### 2.1 Pipeline Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          runts Transpilation Pipeline                    │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐           │
│  │  TS/TSX  │───▶│  Parser  │───▶│ Analyzer │───▶│   HIR   │           │
│  │  Source  │    │  (tsup)  │    │          │    │         │           │
│  └──────────┘    └──────────┘    └──────────┘    └────┬─────┘           │
│                                                       │                 │
│                       ┌───────────────────────────────┤               │
│                       │                               ▼               │
│                       │    ┌──────────────────────────────────────┐   │
│                       │    │          Code Generators            │   │
│                       │    ├──────────────────────────────────────┤   │
│                       │    │  ComponentGen  RouteGen  HookGen     │   │
│                       │    │  SignalGen    IslandGen  MiddleGen   │   │
│                       │    └──────────────────┬───────────────────┘   │
│                       │                       ▼                       │
│                       │              ┌──────────────┐                │
│                       │              │ Rust Source │                │
│                       │              └──────┬───────┘                │
│                       │                     │                        │
│                       ▼                     ▼                        │
│              ┌─────────────────┐    ┌─────────────────┐             │
│              │   Interpreter   │    │  Cargo Build    │             │
│              │   (Dev Mode)    │    │  (Prod Mode)    │             │
│              └─────────────────┘    └─────────────────┘             │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Parser Layer

**Location**: `src/transpile/parser.rs`

The parser handles a **TypeScript subset only**, emitting a High-Level IR (HIR) that captures semantic meaning rather than raw syntax.

#### Supported Parse Rules

```rust
// Parser produces HIR::Module with:
// - ModuleItem::Import - Import declarations
// - ModuleItem::Export - Export declarations  
// - ModuleItem::Decl - Declarations (functions, variables, types)

// Expression types (src/transpile/hir.rs)
pub enum Expr {
    Ident(String),
    String(String),
    Number(f64),
    Boolean(bool),
    Array { elems: Vec<Option<Expr>> },
    Object { props: Vec<ObjectProp> },
    Bin { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    JSX(JSXExpr),
    Arrow { params: Vec<Param>, body: Box<Stmt> },
    // ... more variants
}
```

#### JSX Parsing Strategy

JSX is parsed into an intermediate form that enables efficient Rust codegen:

```rust
pub struct JSXExpr {
    pub opening: JSXOpening,
    pub children: Vec<JSXChild>,
}

pub struct JSXOpening {
    pub name: JSXName,
    pub attrs: Vec<JSXAttr>,
    pub self_closing: bool,
}

// JSXName variants:
// - Ident("div") - HTML element
// - Member { object: "Foo", property: "Bar" } - <Foo.Bar />
// - Dynamic(Box<Expr>) - <{tag} />
```

### 2.3 Semantic Analysis

**Location**: `src/transpile/analyzer.rs`

The analyzer performs:

1. **Type Inference**: Basic type checking without full TypeScript inference
2. **Component Detection**: PascalCase functions → components
3. **Island Detection**: Files in `islands/` → interactive components
4. **Route Detection**: Files in `routes/` → route handlers
5. **Hook Usage**: Track `useState`, `useEffect`, etc.

#### Analysis Rules

```rust
pub struct AnalysisContext {
    // Component stack (for nested component detection)
    components: Vec<String>,
    
    // Hook tracking (for rule enforcement)
    hook_calls: Vec<HookCall>,
    
    // Import resolution
    imports: HashMap<String, ImportInfo>,
    
    // Errors and warnings
    errors: Vec<AnalysisError>,
}
```

#### Analysis Checks

| Check | Severity | Action |
|-------|----------|--------|
| Hook called conditionally | Error | Fail transpilation |
| Hook called in loop | Error | Fail transpilation |
| Unknown type reference | Warning | Use `serde_json::Value` |
| Unused import | Warning | Strip |
| Complex generic nesting | Warning | Simplify |
| Class component usage | Error | Suggest function component |

### 2.4 HIR (High-Level IR)

**Location**: `src/transpile/hir.rs`

The HIR is the canonical intermediate representation:

```rust
/// Top-level module
pub struct Module {
    pub source: String,
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}

/// Module-level items
pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

/// Declarations
pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
}

/// Function declaration (captures Fresh-style patterns)
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Option<Block>,
    pub is_async: bool,
    pub is_generator: bool,
    pub generics: Vec<GenericParam>,
}

/// Route handler pattern (Fresh-specific)
pub struct HandlerPattern {
    pub methods: Vec<HttpMethod>,
    pub params: Vec<Param>,
    pub body: Block,
}
```

### 2.5 Code Generation

**Location**: `src/transpile/codegen.rs`

#### Component Generation

```typescript
// Input: TSX Component
export default function Counter({ initial = 0 }: Props) {
  const [count, setCount] = useState(initial);
  return <div>{count}</div>;
}
```

```rust
// Output: Rust Component
#[component]
pub fn counter(initial: i32) -> VNode {
    let (count, set_count) = use_state(initial);
    html! {
        <div>{ count }</div>
    }
}
```

#### Hook Translation

| TS Hook | Rust Equivalent |
|---------|-----------------|
| `useState<T>(init)` | `use_state(init)` |
| `useEffect(fn, deps)` | `use_effect(\|\| fn())` |
| `useRef<T>(init)` | `use_ref(init)` |
| `useMemo(fn, deps)` | `use_memo(\|\| fn())` |
| `useCallback(fn, deps)` | `use_callback(\|\| fn)` |
| `useContext(Ctx)` | `use_context::<Ctx>()` |

#### JSX → html! Macro

The `html!` macro provides compile-time JSX-like syntax:

```rust
html! {
    <div class="container">
        <h1>{ title }</h1>
        <p>{ description }</p>
        { if show { html!(<Child />) } else { html!(<Empty />) } }
        { items.iter().map(|item| html!(<li key={item.id}>{ item.name }</li>)) }
    </div>
}
```

### 2.6 Route Generation

**Location**: `src/transpile/routegen.rs`

#### File Path → Route Pattern

| File | URL Path | Handler |
|------|----------|---------|
| `routes/index.tsx` | `/` | GET |
| `routes/blog/index.tsx` | `/blog` | GET |
| `routes/blog/[slug].tsx` | `/blog/:slug` | GET |
| `routes/api/[...path].tsx` | `/api/*` | GET, POST, etc. |

#### Handler Translation

```typescript
// routes/blog/[slug].tsx
export const handler = {
    GET: async (req, ctx) => {
        const post = await getPost(ctx.params.slug);
        return ctx.render({ post });
    }
};
```

```rust
// Generated: src/gen/blog/[slug].rs
pub async fn blog_slug_GET(
    req: Request,
    params: BlogSlugParams,
) -> impl IntoResponse {
    let post = get_post(&params.slug).await;
    html! {
        <article>
            <h1>{ &post.title }</h1>
            <div>{ &post.content }</div>
        </article>
    }
}
```

### 2.7 Island Generation

**Location**: Generated as separate modules with hydration support

#### Island Detection

```rust
// islands/Counter.tsx → island module
// Islands are identified by:
// 1. Location in `islands/` directory
// 2. Default export (component function)
// 3. Hook usage (interactive behavior)
```

#### Island Bundle

Each island generates:
1. **Rust SSR component**: Server-side rendering
2. **JS hydration script**: Client-side interactivity
3. **Props serialization**: Safe props passing

```rust
// Generated island module
pub mod counter {
    #[component]
    pub fn counter(initial: i32, step: i32) -> VNode {
        // SSR version
        html!(<div class="island-counter" data-props={serde_json::to_string(...)}>
            {/* Server-rendered HTML */}
        </div>)
    }
}

// Islands are rendered with placeholders:
// <div data-island="counter" data-props="{...}"></div>
```

---

## Part 3: Runtime Architecture

### 3.1 Dual-Mode Design

```
┌─────────────────────────────────────────────────────────────┐
│                    runts Runtime Modes                       │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────┐     ┌─────────────────────┐        │
│  │   Dev Mode (HIR)     │     │  Prod Mode (Rust)   │        │
│  ├─────────────────────┤     ├─────────────────────┤        │
│  │  Interpreter:        │     │  Compiled:          │        │
│  │  - Parse TSX → HIR   │     │  - Full AOT compile │        │
│  │  - Execute HIR       │     │  - Native binary   │        │
│  │  - Hot reload <100ms │     │  - Leptos signals  │        │
│  │                      │     │  - Zero-overhead   │        │
│  │  Trade-offs:         │     │                    │        │
│  │  - Slower runtime    │     │  Trade-offs:       │        │
│  │  - Instant reload    │     │  - Slower compile  │        │
│  │  - Full debugging    │     │  - Fastest runtime │        │
│  └─────────────────────┘     └─────────────────────┘        │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 Dev Mode Runtime

**Location**: `src/runtime/interpreter.rs`

The HIR interpreter executes code without compilation:

```rust
pub struct Interpreter {
    components: HashMap<String, ComponentDef>,
    handlers: HashMap<String, HandlerDef>,
    // ... execution state
}

impl Interpreter {
    /// Execute route handler
    pub fn execute_handler(
        &self,
        path: &str,
        params: HashMap<String, String>
    ) -> Result<serde_json::Value, String>;
    
    /// Render component to HTML
    pub fn render_component(&self, name: &str, props: Value) -> String;
}
```

#### Interpretation Strategy

1. **HIR Execution**: Direct evaluation of HIR nodes
2. **VNode Construction**: Build virtual DOM tree
3. **HTML Serialization**: Convert VNode → HTML string
4. **Island Hydration**: Generate client JS for islands

#### Hook Implementation (Interpreter)

```rust
fn call_function(&self, callee: &Value, args: &[Value], ctx: &mut EvalContext) -> Result<Value, String> {
    match callee {
        Value::Function(name) => match name.as_str() {
            "useState" => {
                let initial = args.first().cloned().unwrap_or(Value::Undefined);
                let state_id = ctx.hooks.state.len();
                ctx.hooks.state.push(initial.clone());
                // Return [value, setter]
                Ok(Value::Array(vec![
                    initial,
                    create_setter(state_id, ctx)
                ]))
            }
            // ... other hooks
        }
    }
}
```

### 3.3 Production Runtime

**Location**: `crates/runts-lib/src/runtime/`

#### Core Runtime Components

```
src/runtime/
├── mod.rs           # Runtime prelude and exports
├── prelude.rs       # Public API (VNode, html!, hooks)
├── component.rs     # Component infrastructure
├── vdom.rs          # Virtual DOM types
├── hooks.rs         # Production hook implementations
├── signals.rs       # Preact-style signals
├── islands.rs       # Island hydration
└── server.rs        # SSR utilities
```

#### VNode Structure

```rust
/// Virtual node for UI rendering
#[derive(Debug, Clone)]
pub enum VNode {
    Element {
        tag: String,
        attrs: Vec<(String, AttrValue)>,
        children: Vec<VNode>,
        key: Option<String>,
    },
    Text(String),
    Fragment(Vec<VNode>),
    Component(ComponentInstance),
}

/// Component instance (lazy evaluation)
#[derive(Clone)]
pub struct ComponentInstance {
    pub name: String,
    pub props: Props,
    pub state: Arc<RwLock<ComponentState>>,
}
```

#### Hook Implementation (Production)

```rust
// hooks.rs - Leptos-inspired hook system

pub fn use_state<T: 'static>(initial: T) -> (Signal<T>, SetSignal<T>) {
    let signal = create_signal(initial);
    let setter = move |new_value| signal.set(new_value);
    (signal, setter)
}

pub fn use_effect<F, D>(f: F, deps: D)
where
    F: Fn() + 'static,
    D: Fn() -> bool + 'static,
{
    // Track dependencies, re-run when changed
    // Cleanup previous effect on re-run
}
```

### 3.4 Signals (Fine-Grained Reactivity)

**Location**: `src/runtime/signals.rs`

Based on Leptos-style fine-grained reactivity:

```rust
// Basic signal
let count = Signal::new(0);
count.set(1);
assert_eq!(count.get(), 1);

// Derived signal (computed)
let doubled = Signal::derive(move || count.get() * 2);

// Signal with notification
let (value, set_value) = create_signal(0);
```

#### Preact Signals Compatibility Layer

```rust
// signals.rs - Preact signals interface
pub struct Signal<T> {
    value: Arc<AtomicPtr<T>>,
    subscribers: Arc<RwLock<HashSet<usize>>>,
}

impl<T: 'static> Signal<T> {
    pub fn new(value: T) -> Self { ... }
    pub fn value(&self) -> T { self.get() }
    pub fn set(&self, new_value: T) { ... }
}

pub fn signal<T: 'static>(initial: T) -> Signal<T> {
    Signal::new(initial)
}

pub fn computed<T: 'static, F: Fn() -> T + 'static>(f: F) -> ReadSignal<T> {
    // Memoized computation
}
```

### 3.5 Islands Architecture

**Location**: `src/runtime/islands.rs`

#### Island Pattern Implementation

```rust
/// Island descriptor
pub struct IslandDef {
    pub name: String,
    pub props_schema: TypeDef,
    pub hydration: HydrationStrategy,
}

/// Hydration strategies
pub enum HydrationStrategy {
    /// Eager: hydrate immediately on load
    Eager,
    /// Lazy: hydrate when visible
    Visible,
    /// Idle: hydrate in browser idle time
    Idle,
    /// Manual: hydrate on interaction
    Manual,
}

/// Island registry for SSR
pub struct IslandRegistry {
    islands: HashMap<String, IslandDef>,
}
```

#### SSR Output

```html
<!-- Islands are rendered as placeholders -->
<div 
    data-island="Counter" 
    data-hydrate="visible"
    data-props='{"initial": 5, "step": 1}'
>
    <!-- SSR HTML (no interactivity) -->
    <div class="counter">
        <p>Count: 5</p>
        <button>+</button>
        <button>-</button>
    </div>
</div>
```

#### Client Hydration

```javascript
// Generated hydration script
import { Counter } from "./islands/counter.js";

class Island {
    constructor(el) {
        this.el = el;
        this.props = JSON.parse(el.dataset.props);
        this.instance = new Counter(this.props);
    }
    
    mount() {
        this.instance.mount(this.el.querySelector('.counter'));
    }
}

// Register islands for hydration
window.__RUNTS_ISLANDS__ = {
    Counter: Island
};
```

---

## Part 4: Server Integration

### 4.1 Axum Integration

**Location**: `src/runtime/server.rs`

```rust
use axum::{
    Router,
    routing::get,
    extract::{Path, State},
};

pub type AppRouter = Router<AppState>;

pub fn create_app(state: AppState) -> AppRouter {
    Router::new()
        .route("/*path", get(handle_route))
        .with_state(state)
}

async fn handle_route(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let html = state.execute_route(&path).await?;
    Html(html).into_response()
}
```

### 4.2 Middleware Pipeline

**Location**: `src/transpile/middlewaregen.rs`

```typescript
// routes/_middleware.ts
import { MiddlewareHandler } from "$fresh/server.ts";

export const handler: MiddlewareHandler = async (req, ctx) => {
    const start = Date.now();
    const res = await ctx.next();
    const duration = Date.now() - start;
    res.headers.set("X-Response-Time", `${duration}ms`);
    return res;
};
```

```rust
// Generated middleware
pub async fn middleware(
    req: Request,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let mut res = next.run(req).await;
    let duration = start.elapsed();
    res.headers_mut().insert(
        "x-response-time",
        format!("{}ms", duration.as_millis())
    );
    res
}
```

### 4.3 Layout System

**Location**: `src/commands/layouts.rs`

#### Layout Detection

```
routes/
├── _layout.tsx          # Root layout
├── _middleware.ts       # Root middleware
├── index.tsx            # /
├── blog/
│   ├── _layout.tsx      # /blog layout
│   ├── _middleware.ts   # /blog middleware
│   ├── index.tsx        # /blog
│   └── [slug].tsx       # /blog/:slug
```

#### Layout Composition

```rust
/// Layout tree builder
pub struct LayoutManager {
    root: LayoutNode,
}

impl LayoutManager {
    /// Build layout tree from routes directory
    pub fn from_routes_dir(dir: &Path) -> Result<Self>;
    
    /// Get layout chain for a path
    pub fn get_layout_chain(&self, path: &str) -> Vec<&LayoutDef>;
    
    /// Render with layouts (outermost first)
    pub fn render_with_layouts(
        &self,
        path: &str,
        content: VNode,
    ) -> VNode;
}
```

---

## Part 5: Performance Targets

### 5.1 Build Performance

| Metric | Target | Current |
|--------|--------|---------|
| Cold start (dev server) | < 500ms | ~800ms |
| Hot reload (single file) | < 50ms | ~20ms |
| Full transpile (prod) | < 5s | ~3s |
| Cargo build (release) | < 30s | ~25s |

### 5.2 Runtime Performance

| Metric | Target | vs Deno Fresh |
|--------|--------|---------------|
| Time to First Byte | < 5ms | 2ms |
| Requests/sec | > 50k | 45k |
| Memory (baseline) | < 2MB | 1.8MB |
| Memory (per request) | < 1KB | 0.8KB |

### 5.3 Bundle Size

| Component | Target | Notes |
|-----------|--------|-------|
| Binary (minimal) | < 1MB | Stripped release |
| Binary (full) | < 5MB | With all features |
| Island JS (avg) | < 500B | Per island |
| Total page (no JS) | < 50KB | SSR only |

---

## Part 6: Roadmap

### Phase 1: MVP (Current) ✅

**Goal**: Core Fresh compatibility for simple apps

- [x] TSX Parser (subset)
- [x] HIR IR
- [x] Component codegen
- [x] Route codegen
- [x] Hooks (useState, useEffect, useRef)
- [x] Dev mode (interpreter)
- [x] Basic SSR
- [x] Islands (static)

### Phase 2: Production Ready

**Goal**: Full Fresh feature parity, production-ready

- [ ] Full type inference
- [ ] All Preact hooks
- [ ] Preact signals (full)
- [ ] Islands hydration (interactive)
- [ ] Middleware (full)
- [ ] Layouts (full)
- [ ] Error boundaries
- [ ] 404/500 pages

### Phase 3: Performance

**Goal**: Match native Rust web frameworks

- [ ] Leptos-style fine-grained reactivity
- [ ] Streaming SSR
- [ ] Edge deployment support
- [ ] WASM target (islands only)
- [ ] Benchmark suite

### Phase 4: Ecosystem

**Goal**: Tooling and DX improvements

- [ ] VS Code extension
- [ ] Debug adapter
- [ ] Hot module replacement (HMR)
- [ ] GraphQL integration
- [ ] Database ORM
- [ ] Auth middleware
- [ ] Testing utilities

---

## Part 7: Trade-offs

### 7.1 Why Not Full TypeScript?

**Trade-off**: Completeness vs. Complexity

Full TypeScript support requires:
- TypeScript compiler (large dependency)
- Complex type inference
- Declaration emit
- Module resolution

**Decision**: Parse subset directly to HIR, strip types at boundary.

### 7.2 Why Not VDOM in Rust?

**Trade-off**: Runtime flexibility vs. Performance

Approaches considered:
1. **Full VDOM**: Most flexible, moderate performance
2. **Fine-grained signals**: Fastest updates, complex implementation
3. **Template compilation**: Fastest SSR, less flexible

**Decision**: Leptos-style signals for production, VNode for SSR compatibility.

### 7.3 Why Not Deno?

**Trade-off**: Compatibility vs. Portability

Deno provides:
- TypeScript natively
- Web APIs
- Package compatibility

**Decision**: Pure Rust eliminates JS runtime dependency, enables true native binaries.

### 7.4 Dev vs Prod Parity

**Trade-off**: Speed vs. Consistency

| Aspect | Dev Mode | Prod Mode |
|--------|----------|----------|
| Execution | Interpreter | Compiled |
| Type safety | Basic | Full |
| Runtime | Rust + TS | Rust only |
| Debugging | Full TS | Rust only |

**Decision**: Dev mode prioritizes DX (instant reload), prod mode prioritizes performance.

---

## Appendix A: Error Messages

### Transpilation Errors

| Code | Message | Fix |
|------|---------|-----|
| E001 | `Class components not supported` | Convert to function component |
| E002 | `Hook called in non-component context` | Move hook call to component |
| E003 | `Hook called conditionally` | Remove conditional |
| E004 | `Dynamic imports not supported` | Use static imports |
| E005 | `Regex literals require /g flag` | Add global flag |
| E006 | `Template literal too complex` | Simplify expression |
| E007 | `JSX spread children not supported` | Use explicit children |
| E008 | `Enum with string values not supported` | Use const object |
| E009 | `Complex generic nesting` | Flatten types |
| E010 | `Namespace not supported` | Use ES modules |

### Runtime Errors

| Code | Message | Fix |
|------|---------|-----|
| R001 | `Hook rule violation: {name}` | Fix hook call site |
| R002 | `Island hydration failed` | Check props serialization |
| R003 | `Route not found: {path}` | Create route file |
| R004 | `Handler error: {message}` | Fix handler code |
| R005 | `Serialization error` | Check props types |

---

## Appendix B: File Structure

```
runts/
├── src/
│   ├── main.rs              # CLI entry
│   ├── cli.rs               # Clap definitions
│   ├── lib.rs               # Library entry
│   ├── config.rs            # Project config
│   ├── transpile/
│   │   ├── mod.rs           # Pipeline
│   │   ├── parser.rs        # TSX → HIR
│   │   ├── hir.rs           # IR definitions
│   │   ├── analyzer.rs      # Semantic analysis
│   │   ├── codegen.rs       # HIR → Rust
│   │   ├── jsx_transformer.rs
│   │   ├── routegen.rs      # Route handlers
│   │   └── middlewaregen.rs  # Middleware
│   ├── runtime/
│   │   ├── mod.rs           # Runtime exports
│   │   ├── prelude.rs       # Public API
│   │   ├── component.rs      # Component infra
│   │   ├── vdom.rs          # Virtual DOM
│   │   ├── hooks.rs         # Hook impls
│   │   ├── signals.rs        # Reactivity
│   │   ├── islands.rs       # Islands
│   │   ├── server.rs        # SSR server
│   │   └── interpreter.rs   # Dev mode
│   └── commands/
│       ├── mod.rs
│       ├── init.rs           # Project init
│       ├── dev.rs            # Dev server
│       ├── build.rs          # Production build
│       ├── routes.rs        # Route utilities
│       └── layouts.rs       # Layout management
├── crates/
│   ├── runts-macros/        # Proc macros (html!, component)
│   ├── runts-lib/           # Runtime library
│   └── runts-client/        # Client hydration
├── examples/
│   └── my-blog/             # Example project
├── tests/
│   └── integration/         # Integration tests
└── SPEC.md                  # This document
```

---

## Appendix C: Reference Implementations

### Counter Island (Fresh-style)

```typescript
// islands/Counter.tsx
import { useState } from "preact/hooks";

interface Props {
  initial?: number;
  step?: number;
}

export default function Counter({ initial = 0, step = 1 }: Props) {
  const [count, setCount] = useState(initial);
  
  return (
    <div class="counter">
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + step)}>+</button>
      <button onClick={() => setCount(count - step)}>-</button>
      <button onClick={() => setCount(initial)}>Reset</button>
    </div>
  );
}
```

### Blog Route (Fresh-style)

```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server.ts";

interface Post {
  title: string;
  content: string;
  publishedAt: string;
}

interface Data {
  post: Post | null;
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext) {
    const slug = ctx.params.slug;
    const post = await db.posts.findBySlug(slug);
    
    return ctx.render({ post });
  }
};

export default function BlogPost({ data }: PageProps<Data>) {
  if (!data.post) {
    return <h1>Post not found</h1>;
  }
  
  return (
    <article>
      <h1>{data.post.title}</h1>
      <time>{data.post.publishedAt}</time>
      <div innerHTML={data.post.content} />
    </article>
  );
}
```

---

**End of Specification**
