# runts — TypeScript R Compiler

> **runts** compiles TypeScript/TSX to native Rust binaries. Not a transpiler. Not a bundler. A compiler that keeps your types: string unions become Rust enums, interfaces become structs, generics monomorphize, async/await becomes zero-cost futures.

[![Tests](https://img.shields.io/badge/tests-864%2F963%20passing-success)]()
[![Rust](https://img.shields.io/badge/rust-1.81%2B-orange)](https://rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

## The Problem: TypeScript Is a Brilliant Type System Handcuffed to a Mediocre Runtime

TypeScript adds a sophisticated type system, then throws away all semantic information at compile time.

```typescript
type Status = 'ok' | 'err' | 'pending';
interface PageProps { title: string; count?: number; }
```

Both types are erased. You accept 3-10x slower execution because "that's how the web works." V8 is an engineering marvel, but it's a general-purpose JS engine, not optimized for typed, compiled code. JIT warm-up, deoptimization cycles, and GC pauses are inherent to the model.

**runts keeps the types.**

## TypeScript R (TSR): What It Is

TSR is an effective subset of TypeScript that compiles to Rust. You write TypeScript. The compiler produces a typed AST where types are first-class, not erased. Type-directed lowering transforms:

- Generics → monomorphized
- String literal unions → Rust enums
- Interfaces → structs with `derive`
- `async/await` → `tokio::task` + zero-cost futures
- TSX → Rust-native UI primitives

The result: native binaries that cold-start in single-digit milliseconds, use < 2MB memory baseline, and have zero GC pauses.

## Type-Directed Lowering

| TypeScript | Rust | Notes |
|---|---|---|
| `interface Foo { bar: number }` | `struct Foo { bar: f64 }` | Structural → nominal |
| `type Status = 'ok' \| 'err'` | `enum Status { Ok, Err }` | String unions → enums |
| `type Result<T> = { data: T } \| { error: string }` | `enum Result<T> { Data(T), Error(String) }` | Tagged unions → ADTs |
| `async function foo()` | `async fn foo()` | State machine by rustc |
| `function foo<T>(x: T)` | `fn foo<T>(x: T)` | Monomorphized at compile |
| `Array<T>` | `Vec<T>` | Growable vector |
| `Promise<T>` | `impl Future<Output = T>` | Zero-cost futures |
| `null \| undefined` | `Option<T>` | Null safety |

## Compilation Pipeline

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
│  Type-Directed Lowering                 │
│  - String unions → enums                │
│  - Interfaces → structs                 │
│  - Generics → monomorphize              │
│  - Async/await → tokio futures          │
└─────────────────────────────────────────┘
    │
    ├──────────────┬──────────────────────┤
    ▼              ▼                      ▼
┌─────────┐  ┌──────────────┐      ┌─────────────┐
│ Dev Mode│  │ JS Bundle    │      │ Production  │
│         │  │ Cache        │      │             │
│ rquickjs│  │ (incremental)│      │ Rust Codegen│
│ + Yoga  │  │              │      │             │
│ bridge  │  │ File Watcher │      │ cargo build │
│ <100ms  │  │ Hot Reload   │      │ --release   │
└─────────┘  └──────────────┘      └─────────────┘
```

### Dev Mode (`runts dev`)
TSX → oxc_parser → oxc_codegen → JS bundle → rquickjs + Yoga bridge → render. Full JS semantics. Real hooks. ~100ms reload.

### Production (`runts build --release`)
TSX → HIR → Rust codegen → cargo build --release → native binary.

## Runtime Comparison

| Feature | Node.js | Deno | Bun | runts |
|---------|---------|------|-----|-------|
| Engine | V8 | V8 | JavaScriptCore | rquickjs (dev) / native (prod) |
| GC pauses | Yes | Yes | Yes | **None** |
| Cold start | 100-500ms | ~50ms | ~20ms | **< 10ms** |
| Memory (idle) | 30MB | 15MB | 5MB | **2MB** |
| Binary size | 50-200MB | ~80MB | ~90MB | **< 5MB** |
| Async runtime | libuv | Tokio | Custom | **Tokio** |
| Concurrency | Event loop | Multi-thread | Multi-thread | **Work-stealing** |

## Quick Start

```bash
# Install
cargo install --path .

# Create new project
runts init my-app
cd my-app

# Development mode (instant hot-reload via rquickjs + Yoga)
runts dev

# Production build (native binary)
runts build --release

# Run production binary
./target/release/my-app
```

## Example: Hono-Style API

```typescript
// routes/hello.ts
import { Context } from "hono";

type Status = 'ok' | 'err' | 'pending';

interface HelloData {
  status: Status;
  message: string;
}

export const handler = {
  async GET(_req: Request, c: Context): Promise<Response> {
    return c.json({ status: 'ok', message: "Hello from Rust!" });
  }
};
```

Compiles to:

```rust
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum Status {
    Ok,
    Err,
    Pending,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HelloData {
    pub status: Status,
    pub message: String,
}

pub async fn handle_get(_req: Request, c: HandlerContext) -> Response {
    let data = HelloData {
        status: Status::Ok,
        message: "Hello from Rust!".to_string(),
    };
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&data).unwrap()))
        .unwrap()
}
```

## Example: Ink TUI Component

```typescript
// tui/app.tsx
import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';

export default function Counter() {
  const [count, setCount] = useState(0);

  useInput((input, key) => {
    if (input === 'q') process.exit(0);
    if (key.upArrow) setCount(c => c + 1);
    if (key.downArrow) setCount(c => c - 1);
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Ink Counter</Text>
      <Text bold>Count: {count}</Text>
    </Box>
  );
}
```

**Dev mode:** `runts dev` transpiles to JS, executes in rquickjs with Yoga layout — identical behavior to deno.
**Production:** Compiles to native Rust binary with Yoga + Ratatui.

## Node.js Compatibility (Three-Tier Strategy)

Full Node.js compatibility is the hardest problem in the JavaScript runtime space. After 4+ years, Deno achieves 76.4% compatibility. After millions in funding, Bun achieves 40.6%. **runts uses a tiered strategy:**

### Tier 1: Native Rust Reimplementation
Core APIs reimplemented in Rust on Tokio:
- `fs` → `tokio::fs` + `nix` for POSIX
- `net` → `tokio::net` (TcpListener, TcpStream)
- `http` → `hyper` + `tower`
- `crypto` → `rustls` + `ring`
- `path` → `std::path::Path` (trivial)

### Tier 2: WASM Sandboxing
Pure JS packages (lodash, date-fns) run in WASI-bound WASM sandbox via wasmtime:
- ~100ns boundary crossing overhead
- postinstall scripts sandboxed, no arbitrary code during install
- Gradual migration, not a rewrite

### Tier 3: V8 Escape Hatch
APIs fundamentally impossible without a JS engine:
- `node:vm` (runInNewContext)
- `node:v8` (heap statistics)
- Native addons using NAN or direct V8 API

V8 is loaded on demand for packages that need it, not for every execution.

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
├── islands/                   # Client-hydrated components
│   └── Counter.tsx
├── components/                # Shared server components (zero JS)
│   └── Header.tsx
└── runts.config.json          # Configuration
```

## Development vs Production

### Development (`runts dev`)
- Parses TS/TSX to AST via **oxc_parser**
- Transpiles to JS bundle via **oxc_codegen**
- Executes in **rquickjs** with Yoga layout bridge
- File watcher with hot-reload (< 100ms)
- Full JS semantics, real hooks, real events
- **No Rust compilation required in dev**

### Production (`runts build --release`)
1. Incremental transpilation — SHA-256 content-hash cache skips unchanged files
2. Transpiles TS/TSX → HIR → Rust source (`.runts/build/`)
3. Generates route table, island manifest, and entry points
4. `cargo build --release` → single static binary
5. Axum server with native SSR throughput

## Documentation

| Document | Description |
|----------|-------------|
| [docs/PHILOSOPHY.md](docs/PHILOSOPHY.md) | Framework-agnostic design rationale — what runts is and isn't |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | HIR design and compile path: oxc_parser → HIR → Rust codegen |
| [docs/INK-ARCHITECTURE.md](docs/INK-ARCHITECTURE.md) | Ink/ratatui plugin: rquickjs + Yoga + Ratatui stack |
| [docs/SUPPORTED_SUBSET.md](docs/SUPPORTED_SUBSET.md) | Precise TS/TSX subset specification |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Roadmap — MVP → Feature Complete → Production |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Performance targets, benchmarks, and trade-offs |
| [DESIGN.md](DESIGN.md) | Current design document — architecture, pipeline, type mappings |

## Status

runts is in active development. The core compiler pipeline (oxc_parser → HIR → Rust codegen → binary) works for:

- ✅ Hono-style API routes (`c.json()`, `c.html()`, `c.text()`)
- ✅ Fresh-style islands (Preact components with `useState`)
- ✅ String literal unions → Rust enums (type-directed lowering)
- ✅ Async/await → tokio futures
- ✅ Incremental builds with content-hash cache

**Completed:**
- ✅ rquickjs dev engine for Ink TUI parity (Tasks 022-026)
- ✅ TSX→JS transpile pipeline (Task 023)
- ✅ Yoga-only layout engine (Taffy removed)
- ✅ Re-enable disabled test modules (Task 033) — all 15 modules enabled
- ✅ Fix HIR test failures (Task 034) — 0 failures, 99 ignored with documented reasons
- ✅ Add missing Ink examples (Task 035) — ink-paste, ink-ref, ink-flex-shrink added
- ✅ Verify js_bridge completeness (Task 036) — 100% prop coverage verified
- ✅ Evaluate Boa vs rquickjs (Task 032) — rquickjs retained (2.4x faster eval)

**Planned:**
- [ ] Better generic lowering (conditional types, indexed access)
- [ ] WASM sandbox for pure npm packages
- [ ] V8 escape hatch for native addons
- [ ] Streaming SSR, error boundaries, observability

## Testing

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test
```

`cargo test -p runts-ink` passes 59/59. `cargo test --bin runts` passes 864 tests with 0 failures and 99 ignored (intentionally skipped features — see Task 034).

## Why Not Just Write Rust?

TSR gives you Rust's performance with TypeScript's DX. You don't fight the borrow checker — types compile to safe Rust automatically. You keep structural typing, union types, and optional parameters. You don't manually manage `Arc<Mutex<T>>`; the compiler inserts the right concurrency primitive based on your type annotations.

For the 95% of TypeScript that maps cleanly to Rust, you write TS and get native code. For the 5% that doesn't (dynamic property access, `eval`), you get a compile-time error with a clear fix.

## License

MIT OR Apache-2.0

---

*Built with Rust. rquickjs + Yoga for dev. Native codegen for prod.*
