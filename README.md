# runts — Fresh/Preact to Native Rust Compiler

> **runts** compiles Fresh/Preact TypeScript/TSX to native Rust binaries with zero external JS runtime dependencies.

A framework that compiles Fresh/Preact TypeScript/TSX to native Rust binaries with zero external JS runtime dependencies.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     runts Architecture                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  User Code (TS/TSX)                                              │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ routes/, islands/, components/, middleware/                  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                              ▼                                   │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                  Transpiler Pipeline                       │  │
│  │  ┌───────────┐  ┌────────────┐  ┌─────────────────────┐  │  │
│  │  │  Parser   │─▶│  Analyzer  │─▶│   Code Generator   │  │  │
│  │  │  (HIR)   │  │ (Semantic) │  │   (Rust source)    │  │  │
│  │  └───────────┘  └────────────┘  └─────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│              ┌───────────────┴───────────────┐                   │
│              ▼                               ▼                   │
│  ┌─────────────────────┐         ┌─────────────────────┐      │
│  │   Development Mode   │         │   Production Mode    │      │
│  │                      │         │                      │      │
│  │  HIR → Interpreter    │         │  Rust codegen →      │      │
│  │  (direct execution)  │         │  cargo build         │      │
│  │                      │         │  (static binary)    │      │
│  │  File watcher        │         │                      │      │
│  │  Instant HMR          │         │  Axum routes        │      │
│  │  (<50ms)            │         │  Islands hydration   │      │
│  └─────────────────────┘         └─────────────────────┘      │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

## Status

| Component | Status | Notes |
|-----------|--------|-------|
| Parser | ✅ Complete | Recursive descent, zero deps |
| HIR | ✅ Complete | Typed AST representation |
| Type Analyzer | ✅ Complete | Type inference, island/route detection |
| Code Generator | ✅ Complete | HIR → Rust source |
| Signals | ✅ Complete | Fine-grained reactivity |
| Hooks | ✅ Complete | useState, useEffect, useRef, useMemo |
| Islands Architecture | ✅ Complete | Hydration modes, registry |
| html! Macro | ✅ Complete | JSX → Rust transform |
| Client JS Runtime | ⚠️ Partial | Structure exists, needs testing |
| HIR Interpreter | ✅ Complete | Full Fresh route handler execution |
| Dev Server | ✅ Complete | File watching, HIR caching, HMR |
| Build Command | ✅ Complete | Transpile + cargo build |

## Quick Start

```bash
# Install
cargo install runts

# Create new project
runts init my-app
cd my-app

# Development mode (instant hot-reload, no Rust recompilation)
runts dev

# Production build
runts build

# Run production binary
./target/release/my-app
```

## Project Structure

```
my-app/
├── routes/                    # File-based routing
│   ├── _middleware.ts          # Global middleware
│   ├── _layout.tsx            # Root layout
│   ├── index.tsx              # GET /
│   └── blog/
│       └── [slug].tsx          # GET /blog/:slug
├── islands/                   # Interactive components
│   └── Counter.tsx             # Hydrated on client
├── components/                # Static components
│   └── Header.tsx
└── runts.config.json          # Configuration
```

## Supported TypeScript/TSX Subset

### Supported Features

| Feature | Syntax | Status |
|---------|--------|--------|
| JSX/TSX | `<div>...</div>`, `<Component />` | ✅ Full |
| Fragments | `<>...</>`, `<Fragment />` | ✅ Full |
| Type annotations | `let x: number = 5` | ✅ Full |
| Interfaces | `interface Foo { a: number }` | ✅ Full |
| Type aliases | `type Foo = Bar \| null` | ✅ Full |
| Arrow functions | `const f = () => {}` | ✅ Full |
| Async/await | `async function foo() {}` | ✅ Full |
| Template literals | `` `hello ${name}` `` | ✅ Full |
| Destructuring | `const { a, b } = obj` | ✅ Full |
| Spread operator | `...rest`, `{...props}` | ✅ Full |
| Optional chaining | `obj?.prop?.nested` | ✅ Full |
| Nullish coalescing | `a ?? b` | ✅ Full |

### Preact Hooks

| Hook | Status |
|------|--------|
| `useState` | ✅ |
| `useEffect` | ✅ |
| `useRef` | ✅ |
| `useMemo` | ✅ |
| `useCallback` | ✅ |
| `useContext` | ✅ |
| `useId` | ✅ |
| `useSignal` | ✅ |
| `useComputed` | ✅ |

### Fresh-Specific

| Feature | Status |
|---------|--------|
| File-based routing | ✅ |
| Layouts | ✅ |
| Middleware | ✅ |
| Islands | ✅ |
| `PageProps` | ✅ |
| `HandlerContext` | ✅ |

### Explicitly Excluded

```typescript
// ❌ Class components
class MyComponent extends Component { }

// ❌ Legacy React APIs
React.memo(Component)
React.forwardRef((props, ref) => ...)
React.Suspense + lazy()

// ❌ TypeScript features
namespace MyNamespace { }
declare module 'x' { }
parameter decorators

// ❌ Complex patterns
eval(), new Function()
Generator functions (yield)
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     runts Architecture                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  User Code (TS/TSX)                                              │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ routes/, islands/, components/, middleware/                  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                              ▼                                   │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                  Transpiler Pipeline                       │  │
│  │  ┌───────────┐  ┌────────────┐  ┌─────────────────────┐  │  │
│  │  │  Parser   │─▶│  Analyzer  │─▶│   Code Generator   │  │  │
│  │  │  (HIR)   │  │ (Semantic) │  │   (Rust source)    │  │  │
│  │  └───────────┘  └────────────┘  └─────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│              ┌───────────────┴───────────────┐                   │
│              ▼                               ▼                   │
│  ┌─────────────────────┐         ┌─────────────────────┐      │
│  │   Development Mode   │         │   Production Mode    │      │
│  │                      │         │                      │      │
│  │  HIR → Interpreter    │         │  Rust codegen →      │      │
│  │  (direct execution)  │         │  cargo build         │      │
│  │                      │         │  (static binary)    │      │
│  │  File watcher        │         │                      │      │
│  │  Instant HMR          │         │  Axum routes        │      │
│  │  (<50ms)            │         │  Islands hydration   │      │
│  └─────────────────────┘         └─────────────────────┘      │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

## Development vs Production

### Development Mode

**Start**: `runts dev`

- Parse TS/TSX to HIR (in-memory)
- Execute HIR directly with interpreter
- File watcher for hot-reload
- **Target**: <50ms from file save to visible update

### Production Mode

**Build**: `runts build`

1. Transpile all TS/TSX → Rust source
2. `cargo build --release`
3. Single static binary (<2MB)

## Example: Route with Handler

```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server";

interface Data {
  title: string;
  content: string;
}

export const handler = {
  GET: async (req: Request, ctx: PageProps<{ slug: string }>) => {
    const { slug } = ctx.params;
    const post = await getPost(slug);
    
    if (!post) {
      return new Response("Not Found", { status: 404 });
    }
    
    return ctx.render({ post });
  }
};

export default function BlogPost({ data }: PageProps<Data>) {
  return (
    <article>
      <h1>{data.post.title}</h1>
      <div>{data.post.content}</div>
    </article>
  );
}
```

## Example: Island Component

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
    <div>
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + step)}>+</button>
    </div>
  );
}
```

## Example: Middleware

```typescript
// routes/_middleware.ts
import { FreshContext } from "$fresh/server";

interface State {
  user?: { id: string; name: string };
}

export async function handler(
  req: Request,
  ctx: FreshContext<State>
) {
  const cookie = req.headers.get("cookie");
  
  if (cookie?.includes("session")) {
    ctx.state.user = { id: "123", name: "Demo" };
  }
  
  const resp = await ctx.next();
  resp.headers.set("X-Custom", "runts");
  
  return resp;
}
```

## Configuration

```json
{
  "server": {
    "port": 8000,
    "host": "127.0.0.1"
  },
  "islands": {
    "hydration": "visible"
  },
  "dev": {
    "port": 8000,
    "open": true,
    "hmr": true
  }
}
```

## Roadmap

### v0.5.0 - MVP (Current)
- [x] Route Handler Exports
- [x] Layout Composition
- [x] Middleware Pipeline
- [x] Page Data
- [x] Client Island JS
- [x] Error Pages

### v0.6.0 - Feature Complete
- [ ] Dynamic JSX tags (`<{tagName} />`)
- [ ] Forward refs
- [ ] Enhanced hooks
- [ ] Asset pipeline
- [ ] Image optimization

### v1.0.0 - Production Ready
- [ ] Production HMR
- [ ] Edge deployment
- [ ] API Routes
- [ ] Database integration
- [ ] Testing utilities

## License

MIT
