# runts — Fresh/Preact to Native Rust Compiler

> **runts** compiles Fresh/Preact TypeScript/TSX to native Rust binaries with **zero external JS runtime dependencies**.

[![Tests](https://img.shields.io/badge/tests-91%2F91%20passing-success)](SPEC.md)
[![Rust](https://img.shields.io/badge/rust-1.81%2B-orange)](https://rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

## What is runts?

runts is a framework and compiler that lets you write **pure Fresh-style Preact TSX** — islands, partial hydration, file-based routing, middleware, hooks — and compiles it to an **efficient native Rust binary**.

- **No V8.** No Deno. No Node.js. No WebAssembly JS engine.
- **Dev mode:** Instant hot-reload via HIR interpreter (< 50ms).
- **Production:** Single static binary via `cargo build --release` (< 2MB).
- **Full islands architecture** with selective client-side hydration.
- **Fine-grained signals** (Leptos-style) for reactive state.

## Architecture

```
User Code (TS/TSX)
    │
    ▼
┌─────────────────────────────────────────┐
│  Parser → HIR → Analyzer → Codegen      │
│  (same pipeline in dev and prod)        │
└─────────────────────────────────────────┘
    │
    ├──────────────┬──────────────────────┤
    ▼              ▼                      ▼
┌─────────┐  ┌──────────────┐      ┌─────────────┐
│ Dev Mode│  │ HIR Cache    │      │ Production  │
│         │  │ File Watcher │      │             │
│ Axum +  │  │ SSE HMR      │      │ cargo build │
│Interpreter│ │ < 50ms      │      │ --release   │
└─────────┘  └──────────────┘      └─────────────┘
```

## Quick Start

```bash
# Install
cargo install --path .

# Create new project
runts init my-app
cd my-app

# Development mode (instant hot-reload, no Rust recompilation)
runts dev

# Production build (native binary)
runts build --release

# Run production binary
./target/release/my-app
```

## Project Structure

```
my-app/
├── routes/                    # File-based routing
│   ├── _middleware.ts         # Global middleware
│   ├── _layout.tsx            # Root layout
│   ├── index.tsx              # GET /
│   └── blog/
│       ├── _layout.tsx        # Blog section layout
│       ├── index.tsx          # GET /blog
│       └── [slug].tsx         # GET /blog/:slug
├── islands/                   # Interactive components (hydrated on client)
│   └── Counter.tsx
├── components/                # Static components
│   └── Header.tsx
├── static/                    # Static assets
├── runts.config.json          # Configuration
└── Cargo.toml                 # Rust dependencies (auto-generated)
```

## Example: Route with Handler

```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server";

interface Data {
  post: { title: string; content: string };
}

export const handler = {
  GET: async (req: Request, ctx: PageProps<{ slug: string }>) => {
    const post = await getPost(ctx.params.slug);
    if (!post) return new Response("Not Found", { status: 404 });
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

## Documentation

| Document | Description |
|----------|-------------|
| [docs/SUPPORTED_SUBSET.md](docs/SUPPORTED_SUBSET.md) | **Precise TS/TSX subset specification** — what's supported, what's excluded, and why |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | **Detailed architecture** — parser, HIR, analyzer, codegen, runtime, dev vs prod |
| [docs/ROADMAP.md](docs/ROADMAP.md) | **Roadmap** — MVP (v0.5) → Feature Complete (v0.6) → Production (v1.0) |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | **Performance targets, benchmarks, and trade-offs** |
| [SPEC.md](SPEC.md) | Legacy technical specification (reference) |

## Supported Subset (Summary)

### ✅ Supported

- JSX/TSX (elements, components, fragments, spread props, conditional rendering)
- All Preact hooks (`useState`, `useEffect`, `useRef`, `useMemo`, `useCallback`, `useReducer`, `useContext`, `useId`, `useSignal`, `useComputed`)
- File-based routing (static, dynamic `[param]`, catch-all `[...path]`, layouts, middleware)
- Async/await, arrow functions, destructuring, template literals, optional chaining, nullish coalescing
- TypeScript interfaces, type aliases, enums, generics (limited)
- Fine-grained signals and effects

### ❌ Excluded

- Class components, `this`, prototypes
- `eval()`, `new Function()`, `with`
- Dynamic `import()`, `require()`
- Conditional types, mapped types, template literal types
- Generators (`function*` / `yield`)
- `Proxy`, `Symbol`, `Reflect`
- Full `try/catch` in render paths

See [docs/SUPPORTED_SUBSET.md](docs/SUPPORTED_SUBSET.md) for the complete specification.

## Development vs Production

### Development (`runts dev`)

- Parses TS/TSX to HIR and executes directly via interpreter
- File watcher with SSE hot-reload (< 50ms)
- Full SSR, islands, layouts, and middleware
- **No Rust compilation required**

### Production (`runts build --release`)

1. Transpiles all TS/TSX → Rust source (`src/gen/`)
2. Generates route table, island manifest, and entry points
3. `cargo build --release` → single static binary
4. Axum server with native SSR throughput

## Performance

| Metric | Target | Status |
|--------|--------|--------|
| Binary size | < 2 MB | ~2.6 MB |
| Cold start | < 5 ms | < 10 ms |
| SSR throughput | > 50k req/s | ~15k req/s |
| Dev hot reload | < 50 ms | < 20 ms |
| Client runtime | < 5 KB gzipped | ~4.2 KB |

See [docs/PERFORMANCE.md](docs/PERFORMANCE.md) for full benchmarks and optimization backlog.

## Testing

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test
```

91 tests passing covering parser, codegen, routing, middleware, signals, hooks, and integration.

## Roadmap

| Phase | Version | Focus | ETA |
|-------|---------|-------|-----|
| MVP | **v0.5** (current) | Core compiler, runtime, islands, dev server | ✅ |
| Feature Complete | v0.6 | Dynamic JSX tags, refs, CSS pipeline, API routes | Q3 2026 |
| Hardening | v0.7 | Streaming SSR, error boundaries, observability | Q4 2026 |
| DX | v0.8 | Fine-grained HMR, error overlay, testing utilities | Q1 2027 |
| Ecosystem | v0.9 | DB integration, deployment adapters, MDX | Q2 2027 |
| Stable | **v1.0** | LTS guarantee, full Fresh compat, <2MB binary | Q3 2027 |

See [docs/ROADMAP.md](docs/ROADMAP.md) for detailed feature lists and decision log.

## License

MIT OR Apache-2.0

---

*Built with Rust. Zero JS runtimes harmed.*
