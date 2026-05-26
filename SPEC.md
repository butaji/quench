# runts Specification

**Status**: Active Development  
**Version**: 0.1.0  
**Target**: Fresh/Preact TSX → Native Rust Binary

---

## Executive Summary

runts is a Fresh/Preact-to-Rust compiler providing framework compatibility using a minimal, precisely-defined subset of TypeScript + TSX.

**Core Principles:**
1. **Correctness first** — Fresh compatibility over performance
2. **Minimal subset** — Ruthlessly exclude rarely-used TS features
3. **Zero external runtimes** — Pure Rust execution
4. **Two modes** — Dev (interpreted, instant reload) / Prod (compiled, native binary)

---

## Part 1: TypeScript/TSX Subset Specification

### 1.1 Design Philosophy

We target **95% coverage of real Fresh/Preact code** by supporting:
- Common patterns exhaustively
- Edge cases with clear fallbacks
- Excluding features with simple workarounds

**Exclusion criteria** (any of):
1. Rarely used in Fresh apps (< 1% usage in real codebases)
2. Semantically complex to transpile correctly
3. Requires runtime support we won't implement
4. Can be expressed simpler with supported features

### 1.2 Supported Syntax

#### ✅ **Full Support**

```
// --- Declarations ---
const/let/var declarations
function declarations (sync + async)
arrow functions
interface declarations (with generics, extends)
type alias declarations (union, intersection, conditional)
import / export (named, default, type-only)
import "path" (side-effect only)

// --- Expressions ---
identifiers, literals (string, number, boolean, null)
template literals (simple: `${expr}` only)
array literals, object literals
binary expressions (+, -, *, /, %, ==, !=, <, >, etc.)
unary expressions (!, -, +)
ternary (a ? b : c)
logical (&&, ||, ??)
spread (...obj, [...arr])
destructuring ({ a, b } = obj)
optional chaining (obj?.prop?.nested)
await expressions
new expressions (limited: `new Date()`, `new Map()`)

// --- Statements ---
if/else, while, for, for...of
return, break, continue
try/catch/finally
switch (with string/number cases only)

// --- Types ---
string, number, boolean, null, undefined, void, never
any, unknown
Array<T>, ReadonlyArray<T>
Record<K,V>, Map<K,V>, Set<T>
Promise<T>
Object types, function types
union, intersection, type references
generics with constraints
```

#### ⚠️ **Supported with Documented Differences**

| Feature | Behavior | Migration |
|---------|----------|----------|
| `class` | Parse only, emit warning | Use function components + closures |
| `enum` (non-const) | Emit warning | Use `as const` objects |
| `namespace` | Parse only, emit warning | Use ES modules |
| `declare` | Strip (no-op) | N/A |
| `module` keyword | Strip (no-op) | Use ES module syntax |
| JSDoc | Strip (no-op) | Use TypeScript types |
| `bigint` literals | `i64` type, warn on overflow | Use `number` |
| RegExp literals | Parse, warn unsupported | Use string methods |

#### ❌ **Explicitly Excluded**

```
// Hard exclusions (parse error)
with statement
eval / Function constructor
arguments object
label statements
 debugger statement
do-while loops
non-const enums

// Soft exclusions (warning + workaround)
class declarations
decorator syntax (@decorator)
namespace modules
module augmentation
declare module
```

### 1.3 JSX/TSX Support

#### ✅ **Full Support**

```tsx
// --- Elements ---
<div>text</div>
<div className="cls">content</div>
<input type="text" value={x} onChange={h} />
<CustomComponent prop={value} />
<>
  <div>A</div>
  <div>B</div>
</>

// --- Expressions in JSX ---
{condition && <Div />}
{condition ? <A /> : <B />}
{array.map(x => <Item key={x.id} {...x} />)}
{"string literal"}
{42}

// --- Event Handlers ---
<button onClick={handler}>Click</button>
<input onInput={e => setValue(e.target.value)} />
<div onMouseEnter={handleHover} />

// --- Props & Children ---
<Comp prop="value" />
<Comp {...spread} />
<Parent>
  <Child />
  {dynamic}
</Parent>

// --- Special Components ---
<island.Counter />          // Explicit island
```

#### ⚠️ **Supported with Constraints**

| Pattern | Constraint | Workaround |
|---------|------------|------------|
| Dynamic tag (`<{tag}>`) | Not supported | `if (tag === 'div')` branches |
| Refs (`ref={r}`) | Not supported | `useRef()` + `querySelector` |
| Portal | Not supported | Manual DOM positioning |
| Context (`<Ctx.Provider>`) | Limited | Props drilling for MVP |
| Suspense | Not supported | Full page loading state |
| ErrorBoundary | Not supported | try/catch in handlers |

### 1.4 Fresh-Specific Features

#### ✅ **Full Support**

```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server";

export default function BlogPost({ params, data }: PageProps) {
  return <h1>{data.title}</h1>;
}

export const handler = {
  GET(req: Request, ctx: HandlerContext) {
    return ctx.render({ title: "Hello" });
  }
};
```

| Feature | Syntax | Notes |
|---------|--------|-------|
| Page props | `PageProps<T>`, `{ params, data }` | Type-safe params |
| Route handlers | `export const handler = { GET, POST, ... }` | Full HTTP methods |
| Middleware | `export default async function` | Per-route or global |
| Islands | `islands/*.tsx` files | Auto-detected |
| Layouts | `_layout.tsx` nesting | Hierarchical |
| Static files | `/static/*` | Tower-http ServeDir |

#### ⚠️ **Supported with Differences**

| Fresh Feature | runts Behavior |
|---------------|----------------|
| `$fresh/` imports | Parsed, types stripped |
| `_middleware.ts` | Same behavior |
| `_app.tsx` | Wraps all routes |
| `static/` | Same |

### 1.5 Preact Hooks

#### ✅ **Full Support**

```tsx
import { useState, useEffect, useRef, useMemo, useCallback } from "preact/hooks";

// useState
const [count, setCount] = useState(0);
setCount(count + 1);

// useEffect
useEffect(() => {
  document.title = `Count: ${count}`;
  return () => console.log("cleanup");
}, [count]);

// useRef
const inputRef = useRef<HTMLInputElement>(null);
inputRef.current?.focus();

// useMemo
const sorted = useMemo(() => 
  items.sort((a, b) => a.localeCompare(b)),
  [items]
);

// useCallback
const handler = useCallback((e: Event) => {
  doSomething(e.target.value);
}, [dep]);
```

| Hook | Signature | Notes |
|------|-----------|-------|
| `useState` | `(init: T \| () => T) -> [T, (T) => void]` | Cloneable types |
| `useEffect` | `(fn: () => void \| cleanup, deps?)` | Cleanup supported |
| `useRef` | `(init?: T) -> { current: T \| null }` | Mutable ref |
| `useMemo` | `(fn: () => T, deps) -> T` | Dep tracking |
| `useCallback` | `(fn: F, deps) -> F` | Function memo |
| `useReducer` | `(reducer, init) -> [S, (A) => void]` | Complex state |

#### ❌ **Not Supported**

- `useContext` (props drilling MVP)
- `useSyncExternalStore` (concurrency)
- `useDeferredValue` (concurrency)
- `useTransition` (concurrency)
- `useLayoutEffect` (use `useEffect`)

### 1.6 Preact Signals

#### ✅ **Full Support**

```tsx
import { signal, computed, effect } from "@preact/signals/core";

// Create signal
const count = signal(0);

// Read value
<div>{count.value}</div>

// Write value
count.value = 42;

// Computed
const doubled = computed(() => count.value * 2);

// Effect
effect(() => console.log("count:", count.value));

// Batch
import { batch } from "@preact/signals-core";
batch(() => {
  count.value = 1;
  count.value = 2;
});
```

---

## Part 2: Architecture

### 2.1 Transpilation Pipeline

```
┌──────────────────────────────────────────────────────────────────────┐
│                        TSX/TS Source                                 │
└──────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  STAGE 1: PARSE                                                        │
│  ───────────────────────────────────────────────────────────────────  │
│  File → HIR (High-level IR)                                          │
│                                                                         │
│  Location: src/transpile/parser.rs (~1700 lines)                     │
│  Type: Hand-written recursive descent                                 │
│  Output: Module with Declarations, Expressions, Types                 │
│                                                                         │
│  Key design decisions:                                                 │
│  • Zero dependencies (no logos, no swc_core)                         │
│  • 1-2 character lookahead for most decisions                         │
│  • Error recovery: skip to next ';' or '}'                            │
│  • JSX is first-class, not an expression                              │
└──────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  STAGE 2: ANALYZE                                                      │
│  ───────────────────────────────────────────────────────────────────  │
│  HIR → Validated HIR + Diagnostics                                     │
│                                                                         │
│  Location: src/transpile/analyzer.rs                                  │
│  Tasks:                                                                 │
│    • Type checking (structural, not sound)                             │
│    • Island detection (islands/*.tsx)                                 │
│    • Route extraction (routes/*.tsx patterns)                          │
│    • Import resolution (type-only stripped)                           │
│    • Hook call validation (rules of hooks)                            │
│    • JSX element validation                                           │
│                                                                         │
│  Output: Vec<Diagnostic> (errors, warnings, hints)                    │
└──────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  STAGE 3: TRANSFORM                                                    │
│  ───────────────────────────────────────────────────────────────────  │
│  Validated HIR → Transformed HIR                                        │
│                                                                         │
│  Location: src/transpile/jsx_transformer.rs                           │
│  Transformations:                                                       │
│    • JSX → html! macro invocations                                    │
│    • className → class_name                                           │
│    • onClick → on_click                                                │
│    • PascalCase → snake_case                                          │
│    • event handlers → closures                                        │
│    • Signals → Signal<T> accessors                                    │
│    • Hooks → runtime hook calls                                       │
└──────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  STAGE 4: GENERATE                                                     │
│  ───────────────────────────────────────────────────────────────────  │
│  Transformed HIR → Rust Source                                         │
│                                                                         │
│  Location: src/transpile/codegen.rs                                    │
│  Output: Formatted Rust source (via rustfmt)                          │
│                                                                         │
│  Type mapping:                                                          │
│    string → String, number → f64, boolean → bool                     │
│    Array<T> → Vec<T>, null/undefined → Option<T>                     │
│    function → Box<dyn Fn(...) -> ... + Send + Sync>                   │
│    interface → struct with serde derives                               │
└──────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  STAGE 5: COMPILE                                                      │
│  ───────────────────────────────────────────────────────────────────  │
│  Rust Source → Native Binary                                           │
│                                                                         │
│  In production:                                                         │
│    • Write to src-gen/ (generated code)                               │
│    • cargo build --release                                            │
│    • LTO + opt-level = "z" for minimal size                          │
│                                                                         │
│  In development: (skip this stage)                                     │
│    • Execute from HIR directly                                        │
│    • Route generation for SSR                                         │
└──────────────────────────────────────────────────────────────────────┘
```

### 2.2 JSX → Rust Transformation

**Input (TSX):**
```tsx
interface Props {
  initial: number;
}

export default function Counter({ initial }: Props) {
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
use runts_lib::runtime::prelude::*;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Props {
    pub initial: f64,
}

#[component]
pub fn counter(initial: f64) -> VNode {
    let (count, set_count) = use_state(|| initial);
    html! {
        <div class_name="counter">
            <p>"Count: " { count }</p>
            <button on_click={ move |_| set_count(count + 1) }>"+"</button>
        </div>
    }
}
```

**Transformations Applied:**

| JSX Pattern | Rust Pattern | Notes |
|-------------|--------------|-------|
| `<div>` | `<div>` | Lowercase preserved |
| `className` | `class_name` | Attr rename |
| `onClick` | `on_click` | Handler rename |
| `PascalCase` | `snake_case` | Component name |
| `{expr}` | `{ expr }` | Brace preservation |
| `"text"` | `"text"` | String literal |
| `onClick={fn}` | `on_click={ fn }` | Closure |
| `onClick={e => fn(e)}` | `on_click={ move \|e\| fn(e) }` | Closure wrap |
| `<Child {...props} />` | `child(props)` | Spread expansion |
| `<>...</>` | Fragment! { ... } | Fragment wrapper |

### 2.3 Islands Architecture

#### 2.3.1 Detection

```
islands/*.tsx     → Island (ship client JS)
components/*.tsx   → Static (zero client JS)
routes/*.tsx      → Route + optional island
```

#### 2.3.2 SSR Output

```html
<!-- Server-rendered island -->
<div 
  data-island="Counter" 
  data-id="island-abc123"
  data-mode="lazy"
  data-props='{"initial":0}'
>
  <button>0</button>
</div>

<!-- Hydrated island -->
<div 
  data-island="Counter" 
  data-id="island-abc123"
  data-mode="lazy"
  data-hydrated="true"
>
  <button onclick="runts_hydrate('island-abc123', ...)">+</button>
</div>
```

#### 2.3.3 Hydration Modes

| Mode | Trigger | Use Case |
|------|---------|----------|
| `eager` | Immediately | Critical UI |
| `visible` | IntersectionObserver | Below fold |
| `lazy` | IntersectionObserver + 100ms | Default |
| `interaction` | First click/focus | Buttons, forms |

---

## Part 3: Development Mode

### 3.1 Zero-Compilation Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                      runts dev                                        │
├──────────────────────────────────────────────────────────────────────┤
│                                                                       │
│   File System          In-Memory Cache        Runtime                  │
│   ──────────          ──────────────        ───────                  │
│                                                                       │
│   routes/*.tsx   ───▶   ModuleCache    ───▶  SSR Handler             │
│   islands/*.tsx  ───▶   (HIR + Errors) ───▶  Island Registry          │
│   components/*  ───▶                    ───▶  Static Components      │
│                                                                       │
│   notify crate          Mutex<HashMap>       Rust-based               │
│   file watching         hot cache            SSR rendering            │
│                                                                       │
└──────────────────────────────────────────────────────────────────────┘
```

**Key insight**: Dev mode NEVER compiles to binary. Instead:
1. Parse TSX to HIR (fast, pure Rust)
2. Execute HIR with Rust runtime (SSR, no JS)
3. Client islands: pre-compiled TypeScript bundles (~12KB each)
4. HMR: WebSocket/SSE triggers page reload, not recompile

### 3.2 Dev Server Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/*` | GET | SSR HTML (from HIR) |
| `/_runts/manifest.json` | GET | Island manifest |
| `/_runts/islands/{name}.js` | GET | Pre-compiled island bundle |
| `/_runts/hmr.js` | GET | HMR client script |
| `/_runts/reload` | GET | SSE for live reload |
| `/static/*` | GET | Static assets |

### 3.3 Hot Reload Flow

```
1. User edits routes/index.tsx
   │
2. notify detects change
   │
3. DevState::transpile() invalidates cache
   │
4. SSE broadcasts reload event
   │
5. Browser receives reload
   │
6. Browser fetches / (SSR renders from HIR)
   │
7. Full page update (no compilation!)
```

**Constraint**: Rust code changes require `runts dev` restart.

---

## Part 4: Production Build

### 4.1 Build Pipeline

```
┌──────────────────────────────────────────────────────────────────────┐
│                       runts build                                     │
├──────────────────────────────────────────────────────────────────────┤
│                                                                       │
│   Phase 1: Discovery (parallel)                                       │
│   ─────────────────────────────────────────────────────────────────   │
│   routes/ → Route manifests                                          │
│   islands/ → Island manifests                                        │
│   components/ → Static components                                    │
│                                                                       │
│   Phase 2: Transpilation (parallel)                                   │
│   ─────────────────────────────────────────────────────────────────   │
│   For each .tsx file:                                                 │
│     1. Parse → HIR                                                    │
│     2. Analyze → Validation                                          │
│     3. Transform → JSX normalized                                    │
│     4. Generate → Rust source                                        │
│                                                                       │
│   Phase 3: Island Bundling                                            │
│   ─────────────────────────────────────────────────────────────────   │
│   1. Collect island manifests                                        │
│   2. Bundle client runtime (~12KB)                                  │
│   3. Output: dist/islands/*.js (hashed names)                        │
│                                                                       │
│   Phase 4: Rust Compilation                                          │
│   ─────────────────────────────────────────────────────────────────   │
│   1. Write generated Rust → src-gen/                                 │
│   2. cargo build --release                                            │
│   3. LTO: fat, codegen-units: 1                                     │
│   4. strip + opt-level = "z"                                         │
│   5. Output: dist/runts-app                                          │
│                                                                       │
└──────────────────────────────────────────────────────────────────────┘
```

### 4.2 Output Structure

```
dist/
├── my-app                 # Linux binary (statically linked)
├── my-app.exe            # Windows binary
├── islands/
│   ├── Counter.a1b2.js   # Hashed island bundle
│   └── TodoList.c3d4.js
├── static/
│   ├── styles.css
│   └── favicon.ico
└── manifest.json          # Island registry
```

---

## Part 5: Performance Targets

### 5.1 Binary Size

| Metric | Target | Measurement |
|--------|--------|-------------|
| Hello World | < 1MB | `runts init hello` |
| Typical App | 2-4MB | Blog + auth |
| Full Fresh | 4-8MB | All features |
| Stripped (debug) | -30% | `strip` binary |

### 5.2 Runtime Performance

| Metric | Target | Notes |
|--------|--------|-------|
| Cold start | < 10ms | Linux x86_64 |
| Request latency (p50) | < 1ms | Simple route |
| Request latency (p99) | < 10ms | SSR with islands |
| Memory (idle) | < 5MB | Baseline |
| Memory (100 req/s) | < 20MB | Under load |
| Island bundle | < 15KB | Gzipped |

### 5.3 Development Experience

| Metric | Target | Notes |
|--------|--------|-------|
| HMR latency | < 100ms | File change → page reload |
| Parse time | < 5ms | Per file |
| Full transpile | < 50ms | Per route |
| Dev server start | < 200ms | `runts dev` |

### 5.4 Trade-offs

| Decision | Trade-off | Rationale |
|----------|-----------|-----------|
| Static linking | Larger binary | Zero runtime deps |
| Custom parser | More code, full control | No swc_core bloat |
| Dev mode: no Rust compile | Instant reload | Rust changes require restart anyway |
| Signals over VDOM diff | Fine-grained reactivity | Preact Signals compatible |

---

## Part 6: Roadmap

### Phase 1: MVP ✅ (COMPLETED)

**Deliverables:**
- [x] Custom TSX parser (~1700 lines, ~85% coverage)
- [x] HIR representation
- [x] Semantic analyzer (island detection, type validation)
- [x] Rust codegen (jsx → html!, types → Rust)
- [x] Runtime: hooks (useState, useEffect, useRef, useMemo, useCallback, useReducer)
- [x] Runtime: signals (Signal, Computed, Effect)
- [x] Runtime: VDOM + html! macro
- [x] Dev server with file watching
- [x] Production build command
- [x] Example app (my-blog)

**Coverage**: ~85% of real Fresh patterns

### Phase 2: Production Ready (IN PROGRESS)

**Required:**
- [ ] Complete TSX parser (remaining 15% edge cases)
- [ ] Type checking pass (soundness, not just structural)
- [ ] Error spans with source locations
- [ ] Full route generation → Axum router wiring
- [ ] Middleware support (`_middleware.ts`)
- [ ] Layout nesting (`_layout.tsx`)
- [ ] Static file serving (`/static/*`)
- [ ] Client-side island hydration (TypeScript runtime)
- [ ] `_app.tsx` global wrapper

**Target**: Q1 2025

### Phase 3: Ecosystem (Q2-Q3 2025)

- [ ] `deno.json` / `fresh.config.ts` compatibility layer
- [ ] Preact compat (`preact/compat` imports)
- [ ] VSCode extension (syntax highlighting)
- [ ] Common patterns: forms, data fetching, error boundaries
- [ ] Testing utilities
- [ ] Documentation site

### Phase 4: Optimization (Q4 2025+)

- [ ] Benchmark suite (criterion)
- [ ] Memory pooling (object arena)
- [ ] Startup time < 5ms
- [ ] WASM output (alternate target)
- [ ] Edge runtime (Cloudflare, Vercel Edge)

---

## Appendix A: File Structure

```
runts/
├── SPEC.md                       # This document
├── README.md                     # Project overview
│
├── crates/
│   ├── runts-lib/               # Runtime library
│   │   └── src/
│   │       ├── prelude.rs       # Re-exports
│   │       ├── component.rs     # #[component] macro
│   │       ├── vdom.rs          # VNode, Fragment
│   │       ├── hooks.rs         # useState, useEffect, etc.
│   │       ├── signals.rs       # Signal, Computed, Effect
│   │       ├── islands.rs       # Island registry
│   │       └── html.rs          # html! macro
│   │
│   └── runts-client/            # Client runtime (TypeScript)
│       └── runtime.ts           # Island hydration
│
├── src/
│   ├── main.rs                  # CLI entry
│   ├── lib.rs                   # Library exports
│   ├── config.rs                # Configuration
│   │
│   ├── transpile/
│   │   ├── parser.rs           # TSX parser (~1700 lines)
│   │   ├── hir.rs              # High-level IR
│   │   ├── analyzer.rs         # Semantic analysis
│   │   ├── codegen.rs          # Rust codegen
│   │   ├── jsx_transformer.rs # JSX normalization
│   │   ├── routegen.rs         # Route handler generation
│   │   └── middlewaregen.rs   # Middleware generation
│   │
│   ├── runtime/
│   │   ├── mod.rs
│   │   ├── hooks.rs
│   │   ├── signals.rs
│   │   ├── islands.rs
│   │   ├── component.rs
│   │   ├── server.rs
│   │   └── html.rs
│   │
│   └── commands/
│       ├── dev.rs              # Dev server
│       ├── build.rs           # Production build
│       └── init.rs           # Project scaffolding
│
├── examples/
│   └── my-blog/               # Example app
│       ├── islands/
│       ├── routes/
│       ├── components/
│       └── static/
│
└── tests/
    ├── integration/
    └── snapshots/
```

---

## Appendix B: Type Mapping Reference

| TypeScript | Rust | Notes |
|------------|------|-------|
| `string` | `String` | UTF-8 owned |
| `number` | `f64` | IEEE 754 double |
| `boolean` | `bool` | Native |
| `null` | `Option<T>` | Via union |
| `undefined` | `()` | Unit type |
| `any` | `serde_json::Value` | JSON |
| `unknown` | `serde_json::Value` | JSON |
| `never` | `!` | Never returns |
| `Array<T>` | `Vec<T>` | Heap-allocated |
| `T[]` | `Vec<T>` | Same |
| `Record<K,V>` | `HashMap<K,V>` | Std |
| `Map<K,V>` | `std::collections::HashMap<K,V>` | |
| `Set<T>` | `std::collections::HashSet<T>` | |
| `Promise<T>` | `tokio::task::JoinHandle<T>` | Async |
| `Date` | `chrono::DateTime<Utc>` | Chrono |
| `function` | `Box<dyn Fn(...) -> ... + Send + Sync>` | Trait |
| `interface` | `struct` + Serde derives | |

---

## Appendix C: Excluded Features Rationale

| Excluded | Reason | Workaround |
|----------|--------|------------|
| `with` | Ambiguous scoping, poor practice | Destructure |
| `eval` | Security, impossible to compile | N/A |
| `class` | Complex runtime semantics | Function + closure |
| Decorators | Stage 2, complex inference | Function wrappers |
| `namespace` | Legacy pattern | ES modules |
| `BigInt` (large) | Precision loss | Use `number` (f64) |
| RegExp | Complex runtime | String methods |
| Generators | Complex control flow | Iterators |

---

*Document Version: 2.0.0*  
*Last Updated: 2025-05-26*
