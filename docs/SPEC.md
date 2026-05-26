# runts Specification

**Version 0.2.0** | Fresh-compatible TSX → Native Rust compiler

---

## Executive Summary

runts compiles a well-defined TypeScript/TSX subset to native Rust binaries, providing full Fresh/Preact framework compatibility without external JS runtimes.

### Core Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          runts Pipeline                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  TypeScript/TSX                                                      │
│       │                                                              │
│       ▼                                                              │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────────────┐ │
│  │ Parser  │───▶│   HIR   │───▶│Analyzer │───▶│ Rust CodeGen    │ │
│  │ 57KB    │    │  AST    │    │         │    │ Components→VNode│
│  └─────────┘    └─────────┘    └─────────┘    └────────┬────────┘ │
│                                                         │          │
│  TSX Source            High-Level IR      Semantic     │          │
│  Zero deps              Normalized AST     Type check   │          │
│                                                         ▼          │
│  ┌────────────────────────────────────────────────────────────────┐│
│  │              Cargo Build (release)                              ││
│  │  LTO + opt-level=z │ Single codegen unit │ Static linking      ││
│  └────────────────────────────────────────────────────────────────┘│
│                                                         │          │
│                                                         ▼          │
│  ┌────────────────────────────────────────────────────────────────┐│
│  │                     Native Binary                               ││
│  │  ~1-5MB binary │ <100ms cold start │ Axum HTTP server         ││
│  └────────────────────────────────────────────────────────────────┘│
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 1. Supported TypeScript/TSX Subset

### 1.1 Core Syntax (FULL SUPPORT)

| Feature | Syntax | Notes |
|---------|--------|-------|
| **JSX/TSX** | `<div>...</div>`, `<Component />` | Components detect by PascalCase |
| **Fragments** | `<>...</>`, `<Fragment />` | Renders multiple children |
| **Type annotations** | `let x: number = 5` | Type info stripped in codegen |
| **Interfaces** | `interface Foo { a: number }` | → Rust struct |
| **Type aliases** | `type Foo = Bar \| null` | Union/intersection support |
| **Generics** | `function foo<T>(x: T): T` | Basic support |
| **Arrow functions** | `const f = () => {}` | → Rust closure |
| **Async/await** | `async function foo() {}` | → Tokio async |
| **Template literals** | `` `hello ${name}` `` | String interpolation |
| **Destructuring** | `const { a, b } = obj` | Object + array |
| **Spread operator** | `...rest`, `{...props}` | Limited to props/arrays |
| **Optional chaining** | `obj?.prop?.nested` | → Rust `?.` or match |
| **Nullish coalescing** | `a ?? b` | → `a.or(b)` |
| **Classes** | `class Foo { ... }` | Data classes only, no inheritance |

### 1.2 Preact Hooks (FULL SUPPORT)

| Hook | Support | Notes |
|------|---------|-------|
| `useState` | ✅ | `let (count, setCount) = use_state(\|\| 0);` |
| `useEffect` | ✅ | `use_effect(\|\| { ... }, [deps]);` |
| `useRef` | ✅ | `let ref = use_ref::<Element>(null);` |
| `useMemo` | ✅ | `use_memo(\|\| expensive(), [deps])` |
| `useCallback` | ✅ | `use_callback(\|\| fn, [deps])` |
| `useContext` | ⚠️ | Limited - Fresh doesn't use context |
| `useReducer` | ⚠️ | Via signals pattern |
| `useLayoutEffect` | ❌ | SSR irrelevant |

### 1.3 Preact Signals (FULL SUPPORT)

| Feature | Syntax | Notes |
|---------|--------|-------|
| `signal()` | ✅ | `let count = signal(0);` |
| `useSignal()` | ✅ | Read signal in component |
| `useSignalEffect()` | ✅ | Reactive effect |
| `computed()` | ✅ | `let doubled = computed(\|\| count * 2);` |
| `batch()` | ✅ | Group updates |

### 1.4 Fresh-Specific APIs (FULL SUPPORT)

| API | Type | Notes |
|-----|------|-------|
| `PageProps<T>` | Interface | Route params + URL |
| `HandlerContext` | Type | Request context |
| `Handler` object | `{ GET?: Handler, POST?: Handler }` | HTTP method handlers |
| `IS_BROWSER` | `boolean` | Runtime detection |
| `_middleware.ts` | Module | Request middleware |
| `_layout.tsx` | Component | Layout wrapper |
| `_app.tsx` | Component | App wrapper |

### 1.5 File-Based Routing (FULL SUPPORT)

| Pattern | File | Handler |
|---------|------|---------|
| `/` | `routes/index.tsx` | `export default` |
| `/blog` | `routes/blog/index.tsx` | `export default` |
| `/blog/[slug]` | `routes/blog/[slug].tsx` | `PageProps<{ slug: string }>` |
| `/api/hello` | `routes/api/hello.ts` | `export const handler` |
| Middleware | `routes/_middleware.ts` | `export default` |
| Layout | `routes/blog/_layout.tsx` | Layout wraps children |

### 1.6 Deliberate Exclusions

```typescript
// ❌ Class components
class MyComponent extends Component { }

// ❌ Legacy React/Preact APIs
React.memo(Component)
React.forwardRef((props, ref) => ...)
createPortal(<Content>, domNode)
React.Suspense + lazy()

// ❌ TypeScript features
namespace MyNamespace { }
enum Color { Red, Green }
declare module 'x' { }
parameter decorators
abstract class

// ❌ Complex patterns
Generator functions (yield)
Iterator protocols
eval(), new Function()
Proxy traps
Symbol.toStringTag

// ❌ Browser-only APIs
window.localStorage (use lib/ polyfill)
fetch (use reqwest crate)
WebSocket (use tokio-tungstenite)
```

### 1.7 Type Mapping

| TypeScript | Rust | Notes |
|------------|------|-------|
| `string` | `String` | |
| `number` | `f64` | JS numbers are floats |
| `boolean` | `bool` | |
| `null` | `()` | Unit type |
| `undefined` | `()` | Unit type |
| `void` | `()` | Unit type |
| `any` | `serde_json::Value` | |
| `unknown` | `serde_json::Value` | |
| `T[]` | `Vec<T>` | |
| `[A, B]` | `(A, B)` | Tuple |
| `{ a: T }` | `struct { pub a: T }` | |
| `A \| B` | `A \| B` | Union |
| `A & B` | `A + B` | Intersection |
| `Record<K, V>` | `HashMap<K, V>` | |
| `Promise<T>` | `impl Future<Output = T>` | |
| `Function` | `Box<dyn Fn(...) -> ...>` | |
| `JSX.Element` | `VNode` | |

---

## 2. Transpilation Pipeline

### 2.1 Parser (Recursive Descent)

**Location:** `src/transpile/parser.rs` (~61KB)

Hand-written recursive descent parser with zero dependencies.

```
Source Text
    │
    ▼
┌─────────────────────────┐
│        Lexer            │
│  Token Stream           │
│  - Keywords             │
│  - Operators             │
│  - Identifiers           │
│  - Literals              │
└──────────┬──────────────┘
           │
           ▼
┌─────────────────────────┐
│        Parser            │
│  Recursive Descent       │
│  - Expression parser     │
│  - Statement parser     │
│  - Type parser           │
│  - JSX parser           │
└──────────┬──────────────┘
           │
           ▼
      HIR Module
```

**Key Parser Features:**
- Full JSX/TSX parsing
- Type annotation parsing
- Template literal support
- Destructuring patterns
- Generic type parameters

### 2.2 High-Level IR (HIR)

**Location:** `src/transpile/hir.rs` (~15KB)

Normalized AST representation for code generation.

```rust
// Core HIR types
pub struct Module { pub items: Vec<ModuleItem> }
pub enum ModuleItem { Import(Import), Export(Export), Decl(Decl) }
pub enum Decl { Function(FunctionDecl), Variable(VariableDecl), Type(TypeDecl) }
pub enum Expr { /* Binary, Unary, Call, JSX, etc. */ }
pub enum Type { String, Number, Boolean, Ref, Union, Array, ... }
```

### 2.3 Semantic Analyzer

**Location:** `src/transpile/analyzer.rs` (~21KB)

Validates and transforms HIR:
- Component detection (PascalCase)
- Hook usage validation
- Type inference for primitives
- Route detection (`[param]` files)
- Island detection (`islands/` files)

### 2.4 Code Generator

**Location:** `src/transpile/codegen.rs` (~38KB)

Transforms HIR to Rust source:

| TS Pattern | Rust Output |
|------------|-------------|
| `<div>Hello</div>` | `html!(<div>"Hello"</div>)` |
| `useState(0)` | `use_state(\|\| 0)` |
| `onClick={handler}` | `on_click: handler` |
| `props.name` | `props.name` |
| `export default` | `pub fn component(...)` |
| `export const handler = {}` | Route handler |
| `const [x, setX] = useState` | `let (x, set_x) = use_state(\|\| ...)` |

---

## 3. Runtime Architecture

### 3.1 Signal System

**Location:** `crates/runts-lib/src/runtime/signals.rs`

Fine-grained reactivity via Rust signals:

```rust
// Signal creation
pub fn signal<T: 'static>(value: T) -> Signal<T>;

// Signal reading
pub fn use_signal<T>(signal: &Signal<T>) -> T;

// Computed signals
pub fn computed<T: 'static, F: Fn() -> T + 'static>(
    f: F
) -> ComputedSignal<T>;
```

**Signal Internals:**
- `Arc<AtomicRefCell<SignalState<T>>>` for interior mutability
- Subscribers tracked via generation counter
- Batch updates via `BATCH_UPDATE` thread-local
- No virtual DOM diffing for signal updates

### 3.2 Hooks Implementation

**Location:** `crates/runts-lib/src/runtime/hooks.rs`

Preact-compatible hook API:

```rust
// useState
pub fn use_state<T, F>(init: F) -> (T, Box<dyn Fn(T)>)
where F: Fn() -> T + 'static, T: 'static;

// useEffect
pub fn use_effect<F, D>(f: F, deps: D)
where F: Fn() -> Option<Box<dyn Fn()>> + 'static,
      D: AsRef<[ConcreteValue]>,;

// useRef
pub fn use_ref<T: 'static>(init: T) -> HookRef<T>;

// useMemo
pub fn use_memo<T: 'static, F: Fn() -> T + 'static>(
    f: F, deps: Rc<[ConcreteValue]>
) -> T;
```

**Hook Storage:**
- `Vec<Box<dyn Any>>` stack per component
- Hook index tracked via `CURRENT_HOOKS` thread-local
- State stored as `Arc<Mutex<Option<T>>>`
- Effects scheduled via `EFFECT_QUEUE` thread-local

### 3.3 Component System

**Location:** `crates/runts-lib/src/runtime/component.rs`

```rust
#[component]
pub fn Counter(props: CounterProps) -> VNode {
    let (count, set_count) = use_state(|| 0);
    
    html! {
        <div>
            <p>{ count }</p>
            <button on_click={ move |_| set_count(count + 1) }>+</button>
        </div>
    }
}
```

**Component Detection:**
1. PascalCase function name (e.g., `MyComponent`)
2. Return type `VNode`
3. Exported from `islands/` or `components/`

### 3.4 html! Macro

**Location:** `crates/runts-macros/src/html.rs` (~22KB)

Compile-time JSX to Rust transformation:

```rust
html! {
    <div class="container">
        <h1>{ title }</h1>
        { children }
    </div>
}
```

**Macro Features:**
- Event handlers: `on_click: handler`, `on_input: handler`
- Props spread: `{...props}` → `..props`
- Conditional: `{cond && <div>}` → match
- Lists: `{items.map(|i| <Item key={i} />)}` → Vec<VNode>

---

## 4. Islands Architecture

### 4.1 Island Definition

**Location:** `crates/runts-lib/src/runtime/islands.rs` (~17KB)

Islands are interactive components hydrated on the client.

```rust
pub enum HydrationMode {
    /// Hydrate immediately
    Eager,
    /// Hydrate when visible
    Lazy,
    /// Hydrate on interaction
    Interaction,
    /// Hydrate when visible
    Visible,
}
```

### 4.2 Island Container HTML

Server renders placeholder:

```html
<div data-island="Counter" 
     data-props="{&quot;initial&quot;:5,&quot;step&quot;:1}">
  <!-- Server-rendered HTML -->
  <p>Count: 5</p>
</div>
```

### 4.3 Client Hydration

**Location:** `crates/runts-client/src/runtime.ts`

Minimal JS for hydration:
- Discover islands via `document.querySelectorAll('[data-island]')`
- Load component bundle
- Mount with props
- Handle hydration modes

---

## 5. Development Mode

### 5.1 Architecture

**Location:** `src/commands/dev.rs`

```
┌─────────────────────────────────────────────────────────────────┐
│                     Development Mode                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  File Change ──────▶ notify watcher ──────▶ Cache Invalidate     │
│       │                                         │               │
│       │                                         ▼               │
│       │                              ┌─────────────────┐         │
│       │                              │  Transpile     │         │
│       │                              │  (in-memory)   │         │
│       │                              └────────┬────────┘         │
│       │                                         │               │
│       │                                         ▼               │
│       │                              ┌─────────────────┐         │
│       │                              │  HIR Cache     │         │
│       │                              └────────┬────────┘         │
│       │                                         │               │
│       │                                         ▼               │
│       │                              ┌─────────────────┐         │
│       │                              │  Runtime        │         │
│       │                              │  Execute        │         │
│       │                              └────────┬────────┘         │
│       │                                         │               │
│       │                                         ▼               │
│       │                              ┌─────────────────┐         │
│       │                              │  Response       │         │
│       └─────────────────────────────▶│  (HTML)         │         │
│                                        └─────────────────┘         │
│                                                                  │
│  ZERO Rust compilation in dev mode                                │
│  Instant hot reload on file change                               │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### 5.2 Hot Module Replacement

1. **File watcher** detects change via `notify` crate
2. **Cache invalidation** - mark file as stale
3. **Re-transpile** - in-memory parser, no disk write
4. **Broadcast reload event** to all connections
5. **Client-side HMR** - fetch updated module, patch runtime

### 5.3 Dev Mode Execution Flow

```
Request
    │
    ▼
┌─────────────────┐
│ Route Match     │  Check file exists, parse route pattern
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Check Cache     │  File modified time vs cache time
└────────┬────────┘
         │
    ┌────┴────┐
    │ Stale?  │
    └────┬────┘
         │
    Yes  │  No
    ┌────┴────────────────┐
    │                     │
    ▼                     ▼
┌─────────┐        ┌─────────────┐
│ Parse   │        │ Use Cache   │
│ & Transpile      │ Return HIR │
└────┬────┘        └──────┬──────┘
     │                    │
     └────────┬───────────┘
              │
              ▼
      ┌───────────────┐
      │ Execute HIR   │
      │ Render VNode  │
      └───────┬───────┘
              │
              ▼
      ┌───────────────┐
      │ Return HTML   │
      └───────────────┘
```

---

## 6. Production Build

### 6.1 Build Pipeline

**Location:** `src/commands/build.rs`

```
┌─────────────────────────────────────────────────────────────────┐
│                     Production Build                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Source Files (.tsx)                                             │
│       │                                                         │
│       ▼                                                         │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Transpile All Files                          │   │
│  │  routes/*.tsx ──▶ gen/routes/*.rs                        │   │
│  │  islands/*.tsx ──▶ gen/islands/*.rs                     │   │
│  │  components/*.tsx ──▶ gen/components/*.rs                │   │
│  └────────────────────────┬─────────────────────────────────┘   │
│                           │                                      │
│                           ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Generate Entry Point                          │   │
│  │  src/main.rs ──▶ Axum router setup                        │   │
│  │  src/lib.rs ──▶ Module exports                           │   │
│  └────────────────────────┬─────────────────────────────────┘   │
│                           │                                      │
│                           ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              cargo build --release                        │   │
│  │                                                          │   │
│  │  • LTO = "fat"                                           │   │
│  │  • codegen-units = 1                                     │   │
│  │  • opt-level = "z"                                       │   │
│  │  • strip = true                                          │   │
│  └────────────────────────┬─────────────────────────────────┘   │
│                           │                                      │
│                           ▼                                      │
│                    Native Binary                                 │
│                    ~1-5MB (stripped)                            │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### 6.2 Cargo Configuration

```toml
[profile.release]
lto = "fat"              # Link-time optimization
codegen-units = 1         # Single unit for max optimization
opt-level = "z"          # Optimize for size
strip = true             # Remove debug symbols
panic = "abort"          # Smaller binary
```

### 6.3 Binary Structure

```
runts-app
├── Static assets    (embedded via include_bytes!)
├── Route handlers   (Axum + Tower)
├── Island registry  (pre-registered components)
└── Client JS        (minimal hydration runtime)
```

---

## 7. Implementation Roadmap

### Phase 0: Foundation (COMPLETE)

| Feature | Status | Notes |
|---------|--------|-------|
| Parser (recursive descent) | ✅ | 61KB, zero deps |
| HIR definition | ✅ | Core AST types |
| Type mapping | ✅ | TS → Rust |
| JSX transformer | ✅ | → html! macro |
| Hooks runtime | ✅ | useState, useEffect, useRef |
| Signals runtime | ✅ | Fine-grained reactivity |
| Islands architecture | ✅ | Hydration modes |
| Server runtime | ✅ | Fresh compatibility |
| html! macro | ✅ | 22KB proc macro |
| CLI (init, dev, build) | ✅ | |
| Example project | ✅ | my-blog |

### Phase 1: Completeness (2-3 weeks)

| Feature | Priority | Notes |
|---------|----------|-------|
| Middleware chain | P0 | `_middleware.ts` support |
| Layout system | P0 | `_layout.tsx`, `_app.tsx` |
| Error boundaries | P1 | `ErrorBoundary` component |
| 404/500 handlers | P1 | `UnknownPageProps` |
| Route params validation | P2 | Type-safe route params |
| More hooks | P2 | useCallback, useMemo improvements |
| Type inference | P2 | Better error messages |

### Phase 2: Quality (3-4 weeks)

| Feature | Priority | Notes |
|---------|----------|-------|
| Error messages | P0 | "Did you mean..." suggestions |
| IDE integration | P1 | LSP for .tsx files |
| Source maps | P1 | Debugging support |
| Better diagnostics | P1 | Type error reporting |
| Documentation | P1 | Migration guide |
| Test suite | P1 | 100+ integration tests |

### Phase 3: Performance (2-3 weeks)

| Feature | Priority | Notes |
|---------|----------|-------|
| Parallel transpilation | P1 | rayon for file processing |
| Incremental builds | P1 | Cache invalidation optimization |
| Binary size reduction | P2 | More aggressive stripping |
| Startup time | P2 | Lazy module loading |
| Memory optimization | P2 | Arena allocators |

### Phase 4: Ecosystem (4+ weeks)

| Feature | Priority | Notes |
|---------|----------|-------|
| Plugin system | P1 | Transform hooks |
| Fresh compatibility layer | P0 | 100% Fresh API |
| Deno compatibility | P2 | Target Deno runtime |
| Edge runtime support | P2 | Cloudflare Workers |
| Testing utilities | P1 | `@runts/testing` |
| Styling solutions | P2 | CSS-in-TS support |

---

## 8. Performance Targets

### 8.1 Build Performance

| Metric | Target | Current |
|--------|--------|---------|
| Cold transpile (100 files) | < 500ms | ~300ms |
| Incremental (1 file) | < 10ms | ~5ms |
| Full release build | < 30s | ~45s |
| Memory usage | < 200MB | ~150MB |

### 8.2 Runtime Performance

| Metric | Target | Notes |
|--------|--------|-------|
| Cold start | < 50ms | Static binary |
| Request latency (p99) | < 10ms | Simple route |
| Throughput | > 50k req/s | Static content |
| Binary size | < 5MB | Stripped release |
| Memory (idle) | < 10MB | Minimal runtime |

### 8.3 Developer Experience

| Metric | Target | Notes |
|--------|--------|-------|
| Hot reload | < 50ms | Per file change |
| Dev server start | < 200ms | No compilation |
| Error feedback | < 100ms | Parse error |
| Type error | < 200ms | Semantic error |

---

## 9. Error Handling

### 9.1 Error Codes

| Code | Category | Example |
|------|----------|---------|
| `E001` | Parse | Unexpected token |
| `E002` | Parse | Unclosed JSX |
| `E003` | Type | Invalid type annotation |
| `E004` | Import | Module not found |
| `E005` | Hook | Invalid hook call |
| `E006` | Component | Missing return type |
| `E007` | Route | Invalid route pattern |
| `E008` | Island | Props serialization failed |
| `E009` | Build | Cargo build failed |
| `E010` | Config | Invalid runts.toml |

### 9.2 Error Format

```json
{
  "error": {
    "code": "E005",
    "message": "Invalid hook call",
    "file": "islands/Counter.tsx",
    "line": 15,
    "column": 10,
    "suggestion": "Hooks must be called at the top level"
  }
}
```

---

## Appendix A: File Structure

```
runts/
├── Cargo.toml                    # Workspace manifest
├── src/
│   ├── main.rs                   # CLI entry point
│   ├── lib.rs                    # Library exports
│   ├── config.rs                 # Config loading
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── init.rs
│   │   ├── dev.rs               # Dev server + HMR
│   │   └── build.rs             # Production build
│   ├── transpile/
│   │   ├── mod.rs               # Pipeline orchestration
│   │   ├── parser.rs            # Recursive descent (~61KB)
│   │   ├── hir.rs               # High-level IR (~15KB)
│   │   ├── analyzer.rs          # Semantic analysis (~21KB)
│   │   ├── codegen.rs           # Rust codegen (~38KB)
│   │   ├── jsx_transformer.rs   # JSX → html! (~21KB)
│   │   ├── routegen.rs          # Route generation (~9KB)
│   │   └── middlewaregen.rs     # Middleware gen (~12KB)
│   └── runtime/
│       ├── mod.rs
│       ├── signals.rs            # Signal implementation
│       ├── hooks.rs             # Hooks implementation
│       ├── component.rs         # Component trait
│       ├── vdom.rs              # Virtual DOM types
│       ├── islands.rs           # Islands architecture
│       └── prelude.rs           # Public API
├── crates/
│   ├── runts-lib/              # Runtime library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── runtime/
│   │   │   └── macros.rs
│   │   └── build.rs
│   ├── runts-client/           # Client runtime (JS)
│   │   └── src/runtime.ts
│   └── runts-macros/           # Proc macros
│       └── src/
│           ├── lib.rs
│           ├── component.rs
│           └── html.rs
├── examples/
│   └── my-blog/                # Example project
├── tests/
│   ├── unit/
│   └── integration/
└── docs/
    ├── SPEC.md                 # This document
    ├── TECHNICAL_SPEC.md       # Detailed implementation
    └── TRANSPILATION_STRATEGY.md
```

---

## Appendix B: Quick Reference

### Component Pattern

```tsx
// islands/Counter.tsx
import { useState } from "preact/hooks";

interface Props {
  initial?: number;
}

export default function Counter({ initial = 0 }: Props) {
  const [count, setCount] = useState(initial);
  
  return (
    <div>
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

### Route Pattern

```tsx
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server.ts";

interface Props {
  slug: string;
}

export default function BlogPost({ params }: PageProps<Props>) {
  return <h1>Post: {params.slug}</h1>;
}
```

### Middleware Pattern

```tsx
// routes/_middleware.ts
import { FreshContext } from "$fresh/server.ts";

export default async function middleware(
  req: Request,
  ctx: FreshContext
) {
  // Add header
  const headers = new Headers(req.headers);
  headers.set("X-Custom", "runts");
  
  return ctx.next();
}
```
