# runts — TypeScript R Compiler

> **runts** compiles an effective subset of TypeScript/TSX to native Rust binaries. Not a transpiler. Not a bundler. A proper compiler pipeline: typed AST → type-directed lowering → Rust source → LLVM-optimized binaries.

[![Tests](https://img.shields.io/badge/tests-111%2F111%20passing-success)](SPEC.md)
[![Rust](https://img.shields.io/badge/rust-1.81%2B-orange)](https://rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

## What is runts?

runts is a **compiler** and **runtime** that lets you write TypeScript/TSX and compile it to a native Rust binary. No V8. No Deno. No Node.js. No WebAssembly JS engine.

- **Type-directed lowering**: TypeScript types inform Rust codegen. String literal unions become Rust enums. Interfaces become structs with `derive`. Generics monomorphize.
- **Custom Tokio-based runtime**: Async/await compiles to zero-cost futures. True work-stealing parallelism across all CPU cores.
- **Dev mode**: Instant hot-reload via HIR interpreter (< 50ms). No Rust recompilation.
- **Production**: Single static binary via `cargo build --release` (5–50 MB).
- **Framework-agnostic**: Fresh, Hono, Express — these are patterns you write in TS/TSX, not parts of the compiler.

## Philosophy

runts is **not** a Fresh clone, **not** a Hono wrapper, and **not** a Node.js replacement. It is a general-purpose compiler for a subset of TypeScript/TSX.

You write TypeScript. The compiler produces a typed AST where types are first-class, not erased. Type-directed lowering transforms your code: generics monomorphize, unions become Rust enums, interfaces become structs, async/await becomes `tokio::task`. The result is a native binary that cold-starts in single-digit milliseconds with no GC pauses.

See [docs/PHILOSOPHY.md](docs/PHILOSOPHY.md) for the full design rationale.

## Architecture

```
TypeScript Source (TS/TSX)
    │
    ▼
┌─────────────────────────────────────────┐
│  oxc_parser → Typed AST                │
│  (types preserved, not erased)          │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  HIR Builder → HIR v2                   │
│  (serializable, interpretable,          │
│   code-generatable)                     │
└─────────────────────────────────────────┘
    │
    ├──────────────┬──────────────────────┤
    ▼              ▼                      ▼
┌─────────┐  ┌──────────────┐      ┌─────────────┐
│ Dev Mode│  │ HIR Cache    │      │ Production  │
│         │  │ (JSON/bincode│      │             │
│ HIR     │  │ incremental) │      │ Rust Codegen│
│Interp   │  │              │      │             │
│+Axum    │  │ File Watcher │      │ cargo build │
│+Hot     │  │ SSE HMR      │      │ --release   │
│Reload   │  │ < 50ms       │      │             │
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
├── routes/                    # File-based routing (convention, not framework)
│   ├── _middleware.ts         # Global middleware
│   ├── _layout.tsx            # Root layout
│   ├── index.tsx              # GET /
│   └── blog/
│       ├── index.tsx          # GET /blog
│       └── [slug].tsx         # GET /blog/:slug
├── islands/                   # Client-interactive components
│   └── Counter.tsx
├── components/                # Shared components
│   └── Header.tsx
├── static/                    # Static assets
└── runts.config.json          # Configuration
```

## Example: Route with Handler (Hono-style)

```typescript
// routes/index.tsx
import { Context } from "hono";

interface HomeData {
  title: string;
  message: string;
}

export const handler = {
  async GET(_req: Request, c: Context): Promise<Response> {
    return c.json({ title: "Home", message: "Hello from runts!" });
  }
};

export default function Home({ data }: { data: HomeData }) {
  return (
    <main>
      <h1>{data.title}</h1>
      <p>{data.message}</p>
    </main>
  );
}
```

## Example: String Union → Rust Enum

```typescript
// types.ts
type Status = 'ok' | 'err' | 'pending';
```

Compiles to:

```rust
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum Status {
    Ok,
    Err,
    Pending,
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
| [docs/PHILOSOPHY.md](docs/PHILOSOPHY.md) | Framework-agnostic design rationale — what runts is and isn't |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Pipeline: oxc_parser → HIR v2 → Rust codegen / interpreter |
| [docs/SUPPORTED_SUBSET.md](docs/SUPPORTED_SUBSET.md) | Precise TS/TSX subset specification |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Roadmap — MVP → Feature Complete → Production |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Performance targets, benchmarks, and trade-offs |
| [SPEC.md](SPEC.md) | Legacy technical specification (reference) |

## Supported Subset (Summary)

### ✅ Supported

- JSX/TSX (elements, components, fragments, spread props, conditional rendering)
- All Preact hooks (`useState`, `useEffect`, `useRef`, `useMemo`, `useCallback`, `useReducer`, `useContext`, `useId`, `useErrorBoundary`, `useSignal`, `useComputed`)
- File-based routing (static, dynamic `[param]`, layouts, middleware)
- Async/await, arrow functions, destructuring, template literals, optional chaining, nullish coalescing
- TypeScript interfaces, type aliases, enums, generics (limited)
- **Type-directed lowering**: string unions → Rust enums, interfaces → structs
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

## Type-Directed Lowering

| TypeScript | Rust | Notes |
|---|---|---|
| `interface Foo { bar: number }` | `struct Foo { bar: f64 }` | Structural → nominal via derive |
| `type Status = 'ok' \| 'err'` | `enum Status { Ok, Err }` | String unions → enums |
| `type ID = string` | `type ID = String` | Type aliases preserved |
| `async function foo()` | `async fn foo()` | State machine by rustc |
| `Array<T>` | `Vec<T>` | Growable vector |
| `Promise<T>` | `impl Future<Output = T>` | Zero-cost futures |
| `null \| undefined` | `Option<T>` | Null safety |

## Development vs Production

### Development (`runts dev`)

- Parses TS/TSX to HIR via **oxc_parser** and executes directly via interpreter
- File watcher with SSE hot-reload (< 50ms)
- Full SSR, islands, layouts, and middleware
- **No Rust compilation required**

### Production (`runts build --release`)

1. **Incremental transpilation** — SHA-256 content-hash cache skips unchanged files
2. Transpiles changed TS/TSX → Rust source (`.runts/build/`)
3. Generates route table, island manifest, and entry points
4. `cargo build --release` → single static binary
5. Axum server with native SSR throughput

## Performance

| Metric | Target | Status |
|--------|--------|--------|
| Binary size | 5–50 MB | **~1.5 MB** (minimal apps) |
| Cold start | < 10 ms | < 10 ms |
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

111 tests passing covering parser, codegen, routing, middleware, signals, hooks, error boundaries, route groups, incremental cache, type-directed lowering, and integration.

## Roadmap

| Phase | Version | Focus | ETA |
|-------|---------|-------|-----|
| MVP | **v0.5** (current) | Core compiler, runtime, islands, dev server | ✅ |
| Type-directed | v0.6 | String unions → enums, better generic lowering | Q3 2026 |
| Hardening | v0.7 | Streaming SSR, error boundaries, observability | Q4 2026 |
| DX | v0.8 | Fine-grained HMR, error overlay, testing utilities | Q1 2027 |
| Ecosystem | v0.9 | DB integration, deployment adapters, MDX | Q2 2027 |
| Stable | **v1.0** | LTS guarantee, <2MB binary | Q3 2027 |

See [docs/ROADMAP.md](docs/ROADMAP.md) for detailed feature lists and decision log.

## License

MIT OR Apache-2.0

---

*Built with Rust. Zero JS runtimes harmed.*
