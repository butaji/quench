# runts Specification

**Status**: Active Development  
**Version**: 0.1.0  
**Target**: Fresh/Preact TSX → Native Rust Binary

---

## Executive Summary

runts is a Fresh/Preact-to-Rust compiler that provides full framework compatibility using a well-defined, minimal subset of TypeScript + TSX. The architecture prioritizes **correctness and Fresh compatibility** over performance, with performance optimization as a secondary concern.

**Key Design Goals:**
- Zero external JS runtimes (no V8, Deno, or WASM JS)
- All code compiles to native Rust via in-memory code generation
- Users write pure Fresh-style Preact TSX with minimal changes
- Dev mode: instant hot-reload via pure runtime execution
- Production: static compilation to efficient native binary

---

## Part 1: Supported TypeScript/TSX Subset

### 1.1 Core Language Features

#### ✅ **Fully Supported**

| Feature | Syntax | Notes |
|---------|--------|-------|
| Type annotations | `let x: number = 5` | All primitive types |
| Interfaces | `interface Foo { bar: string }` | With generics |
| Type aliases | `type Foo = Bar \| Baz` | Union, intersection, mapped |
| Generics | `function fn<T>(x: T): T` | Constraints, defaults |
| Arrow functions | `const fn = () => {}` | With return type inference |
| Async/await | `async function fn()` | Returns `Future<Output=T>` |
| Destructuring | `const { a, b } = obj` | Object & array patterns |
| Spread operator | `...rest` | Objects, arrays, props |
| Template literals | `` `hello ${name}` `` | Simple expressions only |
| Optional chaining | `obj?.prop?.nested` | `?.` and `?[]` |
| Nullish coalescing | `a ?? b` | Converts to `Option<T>` |
| Enum (const) | `const enum E { A, B }` | Inlined at compile time |
| Import/Export | `import { x } from 'y'` | Named, default, type-only |
| Default exports | `export default fn` | Function or expression |

#### ⚠️ **Partially Supported**

| Feature | Status | Workaround |
|---------|--------|------------|
| Class declarations | Parse only, not execute | Use component functions |
| `new` expressions | Limited support | Factory functions instead |
| `typeof` operator | Limited | Type inference preferred |
| Symbol | Limited | `()` placeholder |
| BigInt | `i64` | Loss of precision for >64-bit |
| RegExp | Not supported | String methods only |
| Generator functions | Not supported | Iterators only |
| Iterators/Yield | Not supported | Use `.iter()` loops |

#### ❌ **Explicitly Excluded**

These features are intentionally excluded to maintain simplicity:

| Feature | Reason | Fresh Equivalent |
|---------|--------|------------------|
| `with` statement | Ambiguous scoping | Destructure objects |
| `eval` | Security issue | N/A |
| `arguments` | Not idiomatic Rust | Rest parameters |
| Labels | Rarely needed | Refactor loops |
| `do-while` | Uncommon | `while` loop |
| `debugger` | Development only | Logging |
| Non-const enums | Runtime overhead | Const enums |
| Decorators (TC39) | Stage 2, complex | Custom macros |
| Namespace modules | Legacy pattern | ES modules |
| JSDoc types | Use TypeScript | Type annotations |

### 1.2 JSX/TSX Support

#### ✅ **Fully Supported**

```tsx
// Simple elements
<div className="container">Hello</div>

// Components (PascalCase)
<Counter initial={0} />

// Fragments
<><div>A</div><div>B</div></>

// Conditional rendering
{condition && <div>Shown</div>}
{condition ? <A /> : <B />}

// List rendering
{items.map(item => (
  <Item key={item.id} {...item} />
))}

// Event handlers
<button onClick={handleClick}>Click</button>

// Children
<Container>
  <Child />
  {variableContent}
</Container>
```

#### ⚠️ **Supported with Constraints**

| Pattern | Constraint | Example |
|---------|------------|---------|
| Dynamic tag | Limited | `<tag>{...}</tag>` → `<DynamicTag>` |
| Refs | Alternative | Use `useRef()` hook |
| Portal | Not supported | DOM overlay instead |
| Context | Limited | Props drilling for now |
| Suspense | Not supported | Full page loading |
| ErrorBoundary | Not supported | try/catch blocks |

### 1.3 Fresh-Specific Features

#### ✅ **Fully Supported**

| Feature | Example | Transpiles To |
|---------|---------|--------------|
| Route handlers | `export const handler = { GET, POST }` | Axum route handlers |
| Page props | `function Page({ data }: PageProps<T>)` | Props struct |
| Middleware | `export const handler = onyxMiddleware()` | Axum middleware |
| Islands | Files in `islands/` | Hydrated components |
| Static routes | `routes/about.tsx` | `/about` |
| Dynamic routes | `routes/blog/[slug].tsx` | `/blog/:slug` |
| Layout routes | `routes/_layout.tsx` | Nested layout wrapper |
| Dynamic imports | `await import()` | Lazy module loading |
| Fresh utilities | `apply`, `HandlerContext`, `Request` | Typed wrappers |

#### ⚠️ **Supported with Differences**

| Fresh Feature | Behavior | Difference from Fresh |
|---------------|----------|----------------------|
| `$fresh/` imports | Parsed but not resolved | Placeholder for type imports |
| `islands/` detection | File-based | Same as Fresh |
| `_middleware.ts` | Supported | Same as Fresh |
| `_app.tsx` | Partial | Global layout only |
| `static/` files | Served via Axum | Same as Fresh |
| `routes/` file structure | Full support | Same as Fresh |

### 1.4 Preact Hooks

#### ✅ **Fully Supported**

```tsx
// useState
const [count, setCount] = useState(0);
setCount(count + 1);

// useEffect
useEffect(() => {
  console.log('mounted');
  return () => console.log('unmounted');
}, [dependency]);

// useRef
const inputRef = useRef<HTMLInputElement>(null);
inputRef.current?.focus();

// useMemo
const expensive = useMemo(() => computeExpensive(a, b), [a, b]);

// useCallback
const handler = useCallback((e: Event) => {
  doSomething(e);
}, [dependency]);
```

#### ⚠️ **Supported with Limitations**

| Hook | Limitation | Workaround |
|------|------------|------------|
| `useReducer` | Single reducer only | Multiple `useState` |
| `useContext` | Not implemented | Props drilling |
| `useImperativeHandle` | Not implemented | Direct DOM access |
| `useLayoutEffect` | Sync only | `useEffect` |
| `useId` | Static IDs only | Manual ID generation |

#### ❌ **Not Supported**

- `useSyncExternalStore` (concurrent features)
- `useDeferredValue` (concurrent features)
- `useTransition` (concurrent features)

### 1.5 Signals (Preact Signals-Compatible)

#### ✅ **Fully Supported**

```tsx
import { signal, computed, effect } from '@preact/signals';

// Signal creation
const count = signal(0);

// Computed
const doubled = computed(() => count.value * 2);

// Effect
effect(() => console.log('count:', count.value));

// Usage in JSX
<div>{count.value}</div>;

// Mutation
count.value = 42;
```

---

## Part 2: Architecture & Transpilation Strategy

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Code                                │
│   routes/*.tsx  islands/*.tsx  components/*.tsx  lib/*.ts       │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    1. PARSE (Stage 1)                          │
│   TSX → High-Level AST (HIR)                                   │
│   - Custom hand-written parser                                  │
│   - No external dependencies                                   │
│   - ~2,500 lines Rust                                         │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    2. ANALYZE (Stage 2)                        │
│   HIR → Validated HIR                                          │
│   - Type checking (structural)                                 │
│   - Island detection                                           │
│   - Route extraction                                           │
│   - Import resolution                                          │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    3. TRANSFORM (Stage 3)                      │
│   Validated HIR → Transformed HIR                              │
│   - JSX → html! macro calls                                    │
│   - Hook calls → State/Effect tracking                         │
│   - Signal usage → Reactive updates                            │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    4. GENERATE (Stage 4)                       │
│   Transformed HIR → Rust Source                                │
│   - In-memory code generation                                  │
│   - rustfmt for formatting                                     │
│   - Static linking                                             │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    5. RUNTIME (Stage 5)                        │
│   Rust Source + runts-lib → Native Binary                       │
│   - Axum/Tower HTTP server                                     │
│   - Fine-grained reactivity                                     │
│   - Islands hydration                                          │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Parser Architecture

**Location**: `src/transpile/parser.rs`

The parser is a hand-written, recursive descent parser optimized for the supported subset:

```
Parser
├── Module
│   ├── Import/Export statements
│   ├── Type declarations (interface, type alias)
│   ├── Function declarations (async supported)
│   ├── Variable declarations (const, let, var)
│   └── Default exports
│
├── Expression Parser (Pratt parser)
│   ├── Assignment (=, +=, -=)
│   ├── Ternary (? :)
│   ├── Logical (||, &&, ??)
│   ├── Comparison (==, !=, <, >, etc.)
│   ├── Arithmetic (+, -, *, /, %)
│   ├── Unary (!, -, +, typeof)
│   ├── Call/Member (obj.method, obj[prop])
│   ├── Primary (ident, literals, grouped)
│   └── JSX (embedded in primary)
│
└── Type Parser
    ├── Primitives (string, number, boolean)
    ├── Object types
    ├── Union/Intersection
    ├── Array/Tuple
    ├── Generic references
    └── Function types
```

**Design Decisions:**
1. **No external dependencies**: Parser is ~2,500 lines of pure Rust
2. **Minimal lookahead**: 1-2 characters for most decisions
3. **Error recovery**: Skip to next statement on error
4. **JSX-first**: JSX parsing is primary concern, not generic TS

### 2.3 Code Generation Strategy

**Location**: `src/transpile/codegen.rs`

#### Type Mapping

| TypeScript | Rust | Notes |
|------------|------|-------|
| `string` | `String` | UTF-8 owned |
| `number` | `f64` | IEEE 754 double |
| `boolean` | `bool` | Native |
| `null` | `Option<T>` | Via union types |
| `undefined` | `()` | Unit type |
| `any` | `serde_json::Value` | JSON serialization |
| `unknown` | `serde_json::Value` | JSON serialization |
| `never` | `!` | Never returns |
| `Array<T>` | `Vec<T>` | Heap-allocated |
| `T[]` | `Vec<T>` | Same as Array |
| `Record<K,V>` | `HashMap<K,V>` | std collections |
| `Map<K,V>` | `std::collections::HashMap<K,V>` | |
| `Set<T>` | `std::collections::HashSet<T>` | |
| `Promise<T>` | `tokio::task::JoinHandle<T>` | Async runtime |
| `Date` | `chrono::DateTime<Utc>` | Chrono crate |
| `function` | `Box<dyn Fn(...) -> ... + Send + Sync>` | Trait objects |
| `interface` | `struct` with `Serialize, Deserialize` | Serde derives |
| `enum` | `enum` with variants | Const enums only |

#### JSX → Rust Transformation

**Input (TSX):**
```tsx
function Counter({ initial }: { initial: number }) {
  const [count, setCount] = useState(initial);
  return (
    <div className="counter">
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

**Output (Rust):**
```rust
#[component]
pub fn counter(initial: i32) -> VNode {
    let (count, set_count) = use_state(|| initial);
    html!(
        <div class_name="counter">
            <p>"Count: " { count }</p>
            <button on_click={ move |_| set_count(count + 1) }>"+"</button>
        </div>
    )
}
```

**Transformations Applied:**
1. `className` → `class_name`
2. `onClick` → `on_click` (camelCase to snake_case)
3. `<Component />` → `component(...)` (PascalCase to snake_case)
4. `{expression}` → `{ expression }` (preserve in template)
5. `"text"` → `"text"` (string literals preserved)
6. Event handlers → closures

### 2.4 Islands Architecture

**Location**: `src/runtime/islands.rs`

```
┌────────────────────────────────────────────────────────────┐
│                    Server-Side Rendering                   │
├────────────────────────────────────────────────────────────┤
│                                                             │
│   Page HTML                                                 │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ <div static-content>...</div>                      │   │
│   │ <island data-id="abc" data-props="{...}">           │   │
│   │   <button data-on-click="handler_abc">+</button>   │   │
│   │ </island>                                          │   │
│   └─────────────────────────────────────────────────────┘   │
│                                                             │
│   Island Registry (server)                                   │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ Counter: { initial: 0, id: "abc", mode: "lazy" }    │   │
│   └─────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────┘
                              │
                              ▼ HTML + Island Manifest
┌────────────────────────────────────────────────────────────┐
│                    Client-Side Hydration                     │
├────────────────────────────────────────────────────────────┤
│                                                             │
│   1. Parse island containers from HTML                      │
│   2. Download island manifest                               │
│   3. Lazy-load island JS bundles                            │
│   4. Mount and hydrate on interaction/viewport              │
│                                                             │
│   Island Modes:                                             │
│   - Eager: Hydrate immediately on load                     │
│   - Lazy: Hydrate when entering viewport (IntersectionObserver)
│   - Interaction: Hydrate on first click/focus              │
│   - Visible: Hydrate when visible (IntersectionObserver)    │
└────────────────────────────────────────────────────────────┘
```

#### Island HTML Output

**Server-rendered:**
```html
<div data-island="Counter" data-id="island-1234-abcd" data-mode="lazy">
    <script type="application/x-runts-island">
        {"initial": 0, "label": "Click me"}
    </script>
    <button>0</button>
</div>
```

**Hydrated:**
```html
<div data-island="Counter" data-id="island-1234-abcd" data-mode="lazy" data-hydrated="true">
    <button data-on-click="runts_handler_island-1234-abcd">+</button>
</div>
```

### 2.5 Runtime Architecture

**Location**: `crates/runts-lib/src/runtime/`

```
runts_lib::runtime
├── prelude.rs          # Re-exports for user convenience
├── component.rs        # Component trait and helpers
├── vdom.rs             # Virtual DOM (VNode, Fragment)
├── hooks.rs            # useState, useEffect, useRef, useMemo
├── signals.rs          # Signal, computed, effect
├── islands.rs          # Island registry and hydration
└── server.rs           # SSR utilities

runts_client (TypeScript)
├── runtime.ts          # Client-side island runtime
└── signals.ts          # Signal synchronization
```

#### Signal System (Rust)

```rust
// Fine-grained reactivity inspired by Preact Signals
pub struct Signal<T> {
    value: RwLock<T>,
    subscribers: RwLock<HashSet<WatcherId>>,
}

impl<T: Clone + 'static> Signal<T> {
    pub fn new(value: T) -> Self;
    pub fn value(&self) -> T;
    pub fn set(&self, value: T);
    pub fn update(&self, f: impl FnOnce(T) -> T);
}

// Computed signals
pub struct Computed<T> {
    signal: Signal<T>,
    derive: Arc<dyn Fn() -> T>,
}

// Effects
pub fn effect(f: impl FnOnce() -> ()) -> EffectHandle;
```

---

## Part 3: Development Mode

### 3.1 Dev Server Architecture

**Location**: `src/commands/dev.rs`

```
┌─────────────────────────────────────────────────────────────┐
│                     runts dev                               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   ┌─────────────┐     ┌─────────────┐     ┌─────────────┐  │
│   │ File Watcher│────▶│  Transpiler │────▶│   In-Memory │  │
│   │   (notify)  │     │   (cached)  │     │    Cache    │  │
│   └─────────────┘     └─────────────┘     └─────────────┘  │
│         │                   │                    │          │
│         │ change            │ transpile          │ lookup   │
│         ▼                   ▼                    ▼          │
│   ┌─────────────────────────────────────────────────────┐  │
│   │              Axum Dev Server (port 8080)             │  │
│   │  ┌────────────────────────────────────────────────┐  │  │
│   │  │  /           →  index_handler (SSR)           │  │  │
│   │  │  /islands/*   →  island_bundles                │  │  │
│   │  │  /_runts/*     →  HMR, manifest                │  │  │
│   │  │  /static/*     →  static_files                  │  │  │
│   │  └────────────────────────────────────────────────┘  │  │
│   └─────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 Hot Module Replacement (HMR)

**Zero-compilation HMR Strategy:**

1. **File Change Detection**
   - Use `notify` crate for file system events
   - Watch: `routes/`, `islands/`, `components/`, `lib/`
   - Debounce: 50ms to batch rapid changes

2. **In-Memory Transpilation**
   - Parse changed file to HIR
   - Re-analyze affected modules
   - Generate Rust source (NOT compiled)
   - Store in memory cache

3. **Runtime Execution**
   - Use interpreted HIR for SSR
   - Client JS bundles are pre-compiled
   - Islands are loaded on-demand

4. **Live Reload**
   - WebSocket or SSE for reload events
   - Client fetches new island manifest
   - No full page reload for static content

**Limitations in Dev Mode:**
- Rust code changes require restart (no live Rust reload)
- Full transpilation pipeline still runs
- Only TypeScript/TSX files support true HMR

### 3.3 Dev Server Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/` | GET | Application HTML with SSR |
| `/_runts/manifest.json` | GET | Island manifest |
| `/_runts/islands/{name}.js` | GET | Island bundle |
| `/_runts/hmr.js` | GET | HMR client script |
| `/_runts/reload` | GET/POST | Reload trigger |
| `/static/*` | GET | Static assets |

---

## Part 4: Production Compilation

### 4.1 Build Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                     runts build                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   Phase 1: Analysis                                         │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  1. Discover all routes, islands, components        │  │
│   │  2. Build dependency graph                         │  │
│   │  3. Extract types for code generation               │  │
│   │  4. Generate island manifest                        │  │
│   └─────────────────────────────────────────────────────┘  │
│                           │                                 │
│                           ▼                                 │
│   Phase 2: Transpilation                                    │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  For each TSX file:                                 │  │
│   │    1. Parse → HIR                                  │  │
│   │    2. Analyze → Validated HIR                      │  │
│   │    3. Transform → JSX → html!                      │  │
│   │    4. Generate → Rust source                       │  │
│   │    5. rustfmt → formatted output                    │  │
│   └─────────────────────────────────────────────────────┘  │
│                           │                                 │
│                           ▼                                 │
│   Phase 3: Island Bundling                                  │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  1. Collect all islands                            │  │
│   │  2. Bundle client-side runtime                     │  │
│   │  3. Minify with esbuild (optional)                │  │
│   │  4. Generate hashed filenames                      │  │
│   │  5. Write to dist/islands/                         │  │
│   └─────────────────────────────────────────────────────┘  │
│                           │                                 │
│                           ▼                                 │
│   Phase 4: Rust Compilation                                 │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  1. Write generated Rust to src-gen/               │  │
│   │  2. cargo build --release                          │  │
│   │  3. Static linking (musl for Alpine)                │  │
│   │  4. Binary output: dist/runts-app                   │  │
│   └─────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 Output Structure

```
dist/
├── runts-app           # Linux x86_64 binary
├── runts-app.exe       # Windows binary
├── islands/
│   ├── Counter.js      # Hashed: Counter.a1b2c3d4.js
│   └── TodoList.js     # Hashed: TodoList.e5f6g7h8.js
├── static/
│   ├── styles.css
│   └── favicon.ico
└── manifest.json       # Island manifest
```

### 4.3 Binary Size Targets

| Component | Target | Notes |
|-----------|--------|-------|
| Minimal binary | < 2MB | Hello World app |
| Typical app | 3-5MB | With Axum, Serde |
| Full Fresh app | 5-10MB | With all runtime |
| Static binary | + 20-30% | musl target |
| Gzipped binary | ~40% of binary | For distribution |

---

## Part 5: Performance Analysis

### 5.1 Benchmarks (Planned)

```
┌─────────────────────────────────────────────────────────────┐
│                    Performance Targets                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   Cold Start Time                                           │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  Target: < 50ms (production binary)                │  │
│   │  Target: < 100ms (with static assets)              │  │
│   └─────────────────────────────────────────────────────┘  │
│                                                             │
│   Throughput (req/s)                                        │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  Target: > 10,000 req/s (simple route)             │  │
│   │  Target: > 5,000 req/s (SSR with islands)          │  │
│   │  Target: > 1,000 req/s (complex component tree)    │  │
│   └─────────────────────────────────────────────────────┘  │
│                                                             │
│   Memory Usage                                              │
│   ┌─────────────────────────────────────────────────────┐  │
│   │  Target: < 5MB baseline                           │  │
│   │  Target: < 50MB under load (100 concurrent)      │  │
│   └─────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Trade-offs

| Aspect | Decision | Rationale |
|--------|----------|-----------|
| Binary size vs features | Accept larger binary | Static linking is intentional |
| Parse speed vs compatibility | Custom parser | No external deps, full control |
| Runtime vs compiled | Prefer compiled | Performance over dev convenience |
| Bundle size vs features | Accept larger bundles | Preact signals for reactivity |
| SSR vs client-only | SSR-first | Fresh compatibility priority |

---

## Part 6: Roadmap

### Phase 1: MVP (Current) ✅

- [x] Custom TSX parser
- [x] HIR representation
- [x] Basic code generation
- [x] Islands architecture
- [x] Signal system
- [x] Dev server with watching
- [x] Axum integration
- [ ] Example application

**Status**: Functional but incomplete. Parser covers ~80% of Fresh syntax.

### Phase 2: Production Ready

- [ ] Complete TSX parser (100% of Fresh patterns)
- [ ] Type checking pass
- [ ] Error messages with source locations
- [ ] Full route generation
- [ ] Middleware support
- [ ] `_middleware.ts` support
- [ ] `_app.tsx` support
- [ ] Static file serving
- [ ] Production build command

**Target**: Q2 2025

### Phase 3: Ecosystem

- [ ] VSCode extension with TSX support
- [ ] `deno.json` / `fresh.config.ts` compatibility
- [ ] Preact compat layer (`preact/compat`)
- [ ] Common patterns (forms, routing, data fetching)
- [ ] Testing utilities
- [ ] Documentation site

**Target**: Q3 2025

### Phase 4: Optimization

- [ ] Benchmark suite
- [ ] Binary size optimization
- [ ] Startup time optimization
- [ ] Memory pool allocation
- [ ] WASM output option (alternate target)
- [ ] Edge runtime (Cloudflare Workers, Vercel Edge)

**Target**: Q4 2025

---

## Part 7: API Reference

### 7.1 CLI Commands

```bash
# Development server
runts dev [options]

# Production build
runts build [options]

# Run production binary
runts start [options]

# Initialize new project
runts init [project-name]
```

### 7.2 Configuration

```typescript
// runts.config.ts (optional)
export default {
  // Server configuration
  server: {
    port: 8080,
    host: '0.0.0.0',
  },
  
  // Islands configuration
  islands: {
    // Default hydration mode
    defaultMode: 'lazy',
    
    // Custom islands path
    dir: './islands',
  },
  
  // Build configuration
  build: {
    // Output directory
    out: './dist',
    
    // Static assets
    static: './static',
    
    // Target triple
    target: 'x86_64-unknown-linux-musl',
  },
  
  // Dev configuration
  dev: {
    // Auto-reload delay (ms)
    reloadDelay: 50,
  },
}
```

### 7.3 Runtime API

```rust
// Component definition
#[component]
pub fn MyComponent(props: MyProps) -> VNode;

// Hooks
pub fn use_state<T>(initial: T) -> (T, impl Fn(T));
pub fn use_effect<F>(f: F) -> ()
where F: FnOnce() -> Box<dyn FnOnce()>;
pub fn use_ref<T>() -> Ref<T>;
pub fn use_memo<T, F>(f: F, deps: &[impl Hash]) -> T;

// Signals
pub fn signal<T>(value: T) -> Signal<T>;
pub fn computed<T, F>(f: F) -> Signal<T>
where F: Fn() -> T;
pub fn effect<F>(f: F) -> EffectHandle
where F: Fn() -> Box<dyn FnOnce()>;

// JSX-like template
html!(<div class_name="container">{"Hello"}</div>)
```

---

## Appendix A: Excluded Features Justification

| Feature | Fresh Equivalent | Why Excluded |
|---------|------------------|--------------|
| `namespace` | ES modules | Legacy pattern, bundler handles |
| JSDoc | TypeScript | Unnecessary complexity |
| `declare` | Implementation files | Build-time only |
| `namespace` export | ES re-exports | Duplicate functionality |
| Const assertions | `as const` | Inference handles this |
| Parameter decorators | Hooks | Different abstraction level |

---

## Appendix B: File Structure

```
runts/
├── Cargo.toml                    # Workspace root
├── SPEC.md                       # This document
├── README.md                     # Project overview
│
├── crates/
│   ├── runts-lib/                # Runtime library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── prelude.rs
│   │   │   ├── component.rs
│   │   │   ├── vdom.rs
│   │   │   ├── hooks.rs
│   │   │   ├── signals.rs
│   │   │   ├── islands.rs
│   │   │   └── server.rs
│   │   └── Cargo.toml
│   │
│   ├── runts-macros/             # Procedural macros
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── component.rs
│   │   │   └── html.rs
│   │   └── Cargo.toml
│   │
│   └── runts-client/             # Client runtime
│       ├── src/
│       │   ├── lib.rs
│       │   └── runtime.ts
│       └── Cargo.toml
│
├── src/
│   ├── main.rs                   # CLI entry point
│   ├── lib.rs                    # Library exports
│   ├── config.rs                 # Configuration
│   ├── transpile/
│   │   ├── mod.rs
│   │   ├── parser.rs            # TSX parser
│   │   ├── analyzer.rs          # Semantic analysis
│   │   ├── codegen.rs           # Rust code generation
│   │   ├── hir.rs               # High-level IR
│   │   └── jsx_transformer.rs   # JSX transformations
│   ├── runtime/                  # Server-side runtime
│   │   ├── mod.rs
│   │   ├── hooks.rs
│   │   ├── signals.rs
│   │   ├── islands.rs
│   │   ├── component.rs
│   │   ├── server.rs
│   │   └── html.rs
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── dev.rs               # Dev server
│   │   ├── build.rs             # Production build
│   │   └── init.rs              # Project scaffolding
│   ├── routegen.rs              # Route handler generation
│   └── middlewaregen.rs         # Middleware generation
│
├── examples/
│   └── my-blog/
│       ├── Cargo.toml
│       ├── islands/
│       ├── routes/
│       ├── components/
│       └── static/
│
├── tests/
│   ├── integration/
│   └── snapshots/
│
└── .cargo/
    └── config.toml
```

---

## Appendix C: Contributing

### Code Style

- **Rust**: `cargo fmt` with default settings
- **TypeScript**: ESLint + Prettier
- **Comments**: Doc comments for public API

### Testing

- Unit tests: Inline `#[test]` modules
- Integration tests: `tests/integration/`
- Snapshot tests: For code generation output

### Commit Format

```
type(scope): description

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

---

*Document Version: 1.0.0*  
*Last Updated: 2025-05-26*
