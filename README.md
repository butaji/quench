# runts — Fresh/Preact to Native Rust

**runts** is a Rust-native compiler that transforms Fresh/Preact TypeScript/TSX into production-ready native binaries. Zero external JS runtimes. No V8, no Deno, no WebAssembly JS.

```
┌─────────────────────────────────────────────────────────────────────┐
│  TSX Source          Rust Source          Native Binary              │
│  ─────────           ───────────         ─────────────              │
│                                                                     │
│  routes/*.tsx   →   HIR → codegen   →   Components, Handlers        │
│  islands/*.tsx  →   Signals/Runtime →   Island hydration (~12KB)     │
│  components/*   →   VDOM types     →   Static HTML rendering        │
│                                                                     │
│  target/release/my-app  (~500KB - 2MB)                             │
└─────────────────────────────────────────────────────────────────────┘
```

## Quick Start

```bash
# Install
cargo install --path .

# Create a new project
runts init my-app
cd my-app

# Development (with hot reload)
runts dev

# Production build
runts build
./target/release/my-app
```

## Why runts?

| | Deno Fresh | Next.js | runts |
|---|-----------|---------|-------|
| Runtime | Deno | Node.js | **None (native)** |
| Binary | N/A | N/A | **~1MB** |
| Cold Start | ~50ms | ~200ms | **~5ms** |
| Memory | ~100MB | ~200MB | **~10MB** |
| Islands | ✅ | ✅ | ✅ |
| TypeScript | Full | Full | **Subset** |

## Features

### Framework Compatibility
- ✅ File-based routing (Fresh-style)
- ✅ Islands architecture (partial hydration)
- ✅ Middleware chain (`_middleware.ts`)
- ✅ Layouts (`_layout.tsx`, `_app.tsx`)
- ✅ Preact hooks (`useState`, `useEffect`, etc.)
- ✅ Preact Signals

### Performance
- ⚡ Native binary compilation (LTO, opt-level=z)
- 🔥 Instant cold start (<10ms)
- 💾 Minimal memory footprint (<10MB idle)
- 📦 Tiny binary size (~500KB - 2MB)

### Developer Experience
- 🔄 Hot module replacement (dev mode)
- 📝 Full TypeScript support
- 🎨 JSX/TSX with great error messages
- 🔍 Source maps for debugging

## Supported TypeScript/TSX

### Core Syntax
```typescript
// Functions, arrow functions, async/await ✅
const greet = async (name: string): Promise<string> => {
  return `Hello, ${name}!`;
};

// Interfaces, type aliases, generics ✅
interface User<T extends string> {
  id: T;
  name: string;
  email: string;
}

// JSX/TSX elements and components ✅
function Button({ label }: Props) {
  return <button class="btn">{label}</button>;
}

// Hooks ✅
const [count, setCount] = useState(0);
useEffect(() => console.log(count), [count]);

// Signals ✅
const value = signal(42);
const doubled = computed(() => value.value * 2);
```

### Excluded (Not Supported)
```typescript
// ❌ Class components - use function components
class MyComponent extends Component { }

// ❌ enums - use const objects
enum Color { Red, Green }  // Use as const instead

// ❌ eval() / new Function() - security risk

// ❌ Decorators - use function wrappers
@decorator class Foo { }
```

See [docs/TECHNICAL_SPEC.md](docs/TECHNICAL_SPEC.md) for the full specification.

## Project Structure

```
my-app/
├── routes/                    # File-based routing
│   ├── _app.tsx              # App wrapper
│   ├── _layout.tsx           # Root layout
│   ├── _middleware.ts        # Global middleware
│   ├── index.tsx             # /
│   └── blog/
│       ├── index.tsx         # /blog
│       ├── _layout.tsx       # /blog layout
│       └── [slug].tsx         # /blog/:slug
├── islands/                   # Interactive components
│   ├── Counter.tsx           # Ships JavaScript
│   └── TodoList.tsx
├── components/                # Static components
│   └── Header.tsx            # Zero JavaScript
├── lib/                       # Utilities
│   └── db.ts
├── static/                    # Static assets
│   └── styles.css
├── Cargo.toml
└── runts.config.json
```

## Architecture

### Transpilation Pipeline

```
TSX Source
    │
    ▼
┌─────────────┐
│   Parser    │  Recursive descent TSX parser
│  (~57KB)    │  Zero dependencies
└──────┬──────┘
       │
       ▼
┌─────────────┐
│    HIR      │  High-level IR
│  (AST norm) │  Normalized for codegen
└──────┬──────┘
       │
       ▼
┌─────────────┐
│  CodeGen    │  Rust source generation
│             │  Components → #[component]
│             │  Hooks → runtime::hook()
│             │  JSX → html! macro
└──────┬──────┘
       │
       ▼
  Rust Source
       │
       ▼
┌─────────────┐
│ Cargo Build │  LTO + opt-level=z
│             │  Single codegen unit
└──────┬──────┘
       │
       ▼
   Binary
```

### Runtime System

```
┌─────────────────────────────────────────────────────────────┐
│                      Server Runtime                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Signals ──────► Fine-grained reactivity                     │
│    │                                                         │
│    ▼                                                         │
│  Hooks ────────► useState, useEffect, useRef, etc.          │
│    │                                                         │
│    ▼                                                         │
│  Components ──► Virtual DOM → HTML                           │
│    │                                                         │
│    ▼                                                         │
│  Islands ─────► Client hydration (minimal JS)                │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## CLI Commands

```bash
runts init <name>      # Create new project
runts dev [path]        # Development server with hot reload
runts build [path]      # Production build
runts add <type> <name> # Generate component files
```

## Examples

### Counter (Island)
```tsx
// islands/Counter.tsx
interface Props {
  initial?: number;
}

export default function Counter({ initial = 0 }: Props) {
  const [count, setCount] = useState(initial);
  
  return (
    <div class="counter">
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

### Blog Post (Route)
```tsx
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server";

interface Data {
  title: string;
  content: string;
}

export default function BlogPost({ params, data }: PageProps & { data: Data }) {
  return (
    <article>
      <h1>{data.title}</h1>
      <div>{data.content}</div>
    </article>
  );
}

export const handler = {
  async GET(req, ctx) {
    const post = await getPost(ctx.params.slug);
    return ctx.render({ title: post.title, content: post.content });
  }
};
```

### Middleware
```tsx
// routes/_middleware.ts
export default async function handler(req: Request, ctx: FreshContext) {
  // Add request ID
  ctx.state.requestId = crypto.randomUUID();
  
  // Continue
  return await ctx.next();
}
```

## Performance

| Metric | Target | Current |
|--------|--------|---------|
| Cold Start | < 10ms | ~5-15ms |
| Binary Size | < 1MB | ~500KB - 2MB |
| Memory (idle) | < 5MB | ~5-10MB |
| Island Bundle | < 15KB | ~12KB |
| Hot Reload | < 100ms | ~50-100ms |

## Documentation

- [Technical Specification](docs/TECHNICAL_SPEC.md) — Architecture, TS subset, pipeline
- [Migration Guide](docs/MIGRATION.md) — Fresh → runts
- [API Reference](docs/API.md) — Generated code API

## Roadmap

- [x] MVP (parser, hooks, signals, islands)
- [ ] Full routing (all HTTP methods)
- [ ] WebSocket HMR
- [ ] Streaming SSR
- [ ] Edge deployment
- [ ] Database integrations

## Contributing

Contributions welcome! Please read the [contributing guide](CONTRIBUTING.md).

## License

MIT OR Apache-2.0
