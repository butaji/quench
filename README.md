# Runie - TypeScript to Rust Compiler Driver

> A compiler driver that makes `*.r.ts` and `*.r.tsx` valid source files for Rust projects with zero runtime overhead.

## Overview

Runie is a compiler driver that parses TypeScript-like syntax (`.r.ts` and `.r.tsx` files) using SWC, validates the zero-overhead subset, and transpiles to Rust source code. The generated Rust code compiles with `rustc` to produce native binaries with zero runtime overhead.

### Key Features

- **Zero-overhead transpilation**: TypeScript → Rust with no runtime dependency
- **Hot reload**: Development builds via cdylib for live reloading
- **Native interop**: Seamless integration with hand-written Rust via `native:` imports
- **Strict subset validation**: Rejects `any`, `class`, `try/catch`, `==`, etc.
- **Ownership inference**: Automatic `&T` / `&mut T` / `T` based on usage patterns
- **JSX support**: Transpiles to Ratatui builder patterns

## Installation

```bash
cargo install --path crates/runie-cli --features binary-runie
```

Or build from source:

```bash
cargo build --release -p runie-cli
```

## CLI Commands

```bash
# Development mode with hot reload
cargo runie dev

# Release build (static binary)
cargo runie build --release

# Type check only
cargo runie check

# Transpile a file to stdout
cargo runie transpile path/to/file.r.ts

# Initialize a new project
cargo runie init --name myproject
```

## Type Mapping

| Runie (TS) | Rust | Notes |
|---|---|---|
| `number` | `f64` | Default |
| `number` (literal) | `i32` | Integer literals |
| `string` | `String` | Heap allocated |
| `string` (literal) | `&str` | Borrowed |
| `boolean` | `bool` | |
| `T \| null` | `Option<T>` | |
| `{ok, value}` / `{ok, error}` | `Result<T, E>` | With `?` operator |

## The Runie Subset

### Forbidden Features

| Feature | Error |
|---|---|
| `any`, `unknown` | Use concrete types |
| `class`, `new`, `this` | Use plain objects |
| `var` | Use `const` or `let` |
| `==`, `!=` | Use `===`, `!==` |
| `try`/`catch`/`throw` | Use `Result<T, E>` |
| `eval`, `with` | Dynamic scoping forbidden |
| `obj[key]` | Use `Map<K,V>` |
| `delete` | Use ownership |
| `for...in` | Use `for...of` |

### Allowed Patterns

```typescript
// Result pattern with ? operator
function divide(a: number, b: number):
  | { ok: true, value: number }
  | { ok: false, error: string }
{
  if (b === 0) {
    return { ok: false, error: "division by zero" };
  }
  return { ok: true, value: a / b };
}

// Usage with early return
function caller(): Result<number, string> {
  const r = divide(10, 2);
  if (!r.ok) {
    return r;  // returns Err
  }
  return { ok: true, value: r.value + 1 };
}
```

## Project Structure

```
myproject/
├── Cargo.toml              # Workspace
├── Runie.toml               # Runie configuration
│
└── crates/
    ├── protocol/           # Shared state trait
    │   └── src/lib.rs
    │
    ├── host/              # Thin binary (~80 lines)
    │   └── src/main.rs
    │
    └── app/               # Hot-reloadable cdylib
        └── src/
            ├── lib.rs     # Hand-written wiring
            ├── main.r.ts  # Entry point
            ├── state.r.ts # State logic
            ├── views/
            │   └── root.r.tsx  # JSX UI
            └── native/    # Hand-written Rust
                └── fast_math.rs
```

## Hot Reload Protocol

1. `cargo runie dev` scans for `.r.ts` / `.r.tsx` files
2. SWC parses → validates subset → generates Rust to `target/Runie-cache/`
3. Compiles to cdylib in `target/debug/`
4. Copies to `target/hot/libapp_<timestamp>.so`
5. Updates `target/hot/.current` symlink atomically
6. Host polls `.current`, unloads old dylib, loads new one
7. `AppState` survives in host heap

## Examples

See `Runie_framework_examples.md` for 12 framework-specific examples showing zero-overhead TS→Rust mappings:

| # | Framework | Pattern |
|---|---|---|
| 1 | Axum | REST API with extractors |
| 2 | Actix-web | Scope-based routing |
| 3 | Ratatui + clap | TUI dashboard |
| 4 | Tauri | Desktop commands |
| 5 | Dioxus | Cross-platform UI |
| 6 | egui | Immediate-mode tools |
| 7 | Leptos | Full-stack reactive |
| 8 | Yew | Component-based WASM |
| 9 | Bevy | ECS game systems |
| 10 | SQLx | Compile-time checked DB |
| 11 | Tonic | gRPC services |
| 12 | Candle | LLM inference |

Source files live in `examples/01_axum_api/` through `examples/12_candle_infer/`.
Tests in `framework_example_tests.rs` verify each example parses and emits key patterns.

## Architecture

```
.r.ts / .r.tsx  ──►  SWC Parser  ──►  Runie Analyzer  ──►  Rust Codegen  ──►  rustc
     │                    │                │                    │
     │                    │                │                    └── target/Runie-cache/
     │                    │                └── borrow check, subset validation
     │                    └── produces TS AST
     └── you edit this
```

### Modules

- **parser**: SWC integration for TypeScript parsing
- **analyzer**: Subset validation + ownership inference  
- **codegen**: TS AST → Rust source transpilation
- **driver**: Orchestration + cargo integration
- **reload**: dylib watcher + host signaler

## Code Quality

- Max 500 lines per file
- Max 40 lines per function
- Complexity ≤ 10 (Clippy)
- `cargo clippy -D warnings`

## License

MIT
