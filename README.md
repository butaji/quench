# runts — Fresh/Preact to Native Rust

**runts** is a Rust-native compiler that transforms Fresh/Preact TypeScript/TSX into production-ready native binaries. Zero external JS runtimes (no V8, Deno, WebAssembly JS).

## Quick Start

```bash
# Install
cargo install --path .

# Create a new project
runts init my-app
cd my-app

# Development
runts dev

# Production build
runts build
cargo run --release
```

## Features

- **Native Rust** — Compiles to a single static binary
- **Islands Architecture** — Zero JS for static content, interactive islands only
- **Fine-grained Reactivity** — Signals-based updates (O(1) per change)
- **Fresh Compatible** — Write Fresh-style code, compile to native
- **TypeScript/TSX** — Full type safety with custom parser
- **Fast Cold Start** — Sub-50ms with static binary

## Architecture

```
TypeScript/TSX → Parser → HIR → CodeGen → Rust Source → Binary
                                       ↓
                              Islands: JS bundles
```

### Why runts?

| | React/Virtual DOM | Deno Fresh | runts |
|---|-------------------|------------|-------|
| Runtime | V8 | Deno | None (native) |
| Cold Start | ~100ms | ~50ms | **~5ms** |
| Binary Size | N/A | N/A | **<5MB** |
| Islands | Partial | ✅ | ✅ |
| TypeScript | Full | Full | Subset |

## Supported Patterns

### Hooks
```typescript
const [count, setCount] = useState(0);
useEffect(() => console.log(count), [count]);
const ref = useRef<HTMLInputElement>(null);
```

### Components
```tsx
export default function Counter({ initial = 0 }: CounterProps) {
  const [count, setCount] = useState(initial);
  
  return (
    <div class="counter">
      <p>{count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

### Islands
```tsx
// islands/Counter.tsx - ships JavaScript
export default function Counter() { ... }

// components/Header.tsx - zero JavaScript
export default function Header() { ... }
```

### Routing
```
routes/
├── index.tsx           → /
├── about.tsx          → /about
├── blog/
│   ├── index.tsx      → /blog
│   └── [slug].tsx     → /blog/:slug
└── _layout.tsx        → Layout wrapper
```

## Project Structure

```
my-app/
├── routes/            # File-based routing
│   ├── _app.tsx       # App wrapper
│   ├── _layout.tsx    # Root layout
│   ├── index.tsx      # Home page
│   └── blog/
│       ├── index.tsx  # /blog
│       └── [slug].tsx # /blog/:slug
├── islands/           # Interactive components
│   ├── Counter.tsx
│   └── TodoList.tsx
├── components/        # Static components
│   └── Header.tsx
├── lib/               # Utilities
│   └── utils.ts
├── static/            # Static assets
│   └── styles.css
├── Cargo.toml
└── runts.config.json
```

## CLI Commands

```bash
runts init <name>      # Create new project
runts dev [path]        # Development server
runts build [path]      # Production build
runts add <type> <name> # Generate files
```

## TypeScript Subset

### Supported
- Functions, arrow functions, async/await
- Interfaces, type aliases, generics
- JSX/TSX elements and components
- useState, useEffect, useRef, useMemo, useCallback
- Signals (signal, useSignal, useComputed)
- File-based routing with dynamic segments

### Excluded
- `enum` (use `as const` unions)
- `namespace` (use ES modules)
- Class components (use function components)
- `eval()` / `new Function()`
- Decorators
- Generators (`yield`)

## Client Runtime

Islands use a minimal JavaScript runtime (~12KB):

```javascript
import { signal, html } from "@runts/runtime";

export function hydrate(container, props) {
  const count = signal(props.initial || 0);
  count.subscribe(() => {
    container.innerHTML = html`
      <div class="counter">
        <p>Count: ${count}</p>
        <button onClick=${() => count.value++}>+</button>
      </div>`;
  });
}
```

## Performance

| Metric | Target | Current |
|--------|--------|---------|
| Cold Start | < 50ms | ~20-50ms |
| Hot Reload | < 100ms | ~50ms |
| Binary Size | < 5MB | TBD |
| Memory (idle) | < 10MB | TBD |
| Island Bundle | < 15KB | ~12KB |

## Roadmap

- [x] MVP (parser, hooks, signals, islands)
- [ ] Full routing (catch-all, handlers)
- [ ] WebSocket HMR
- [ ] Streaming SSR
- [ ] Edge deployment

## License

MIT OR Apache-2.0
