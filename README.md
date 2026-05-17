# Rune - TypeScript to Rust Compiler Driver

> A compiler driver that makes `*.r.ts` and `*.r.tsx` valid source files for Rust projects with zero runtime overhead.

## Overview

Rune is a compiler driver that parses TypeScript-like syntax (`.r.ts` and `.r.tsx` files) using SWC, validates the zero-overhead subset, and transpiles to Rust source code. The generated Rust code compiles with `rustc` to produce native binaries with zero runtime overhead.

### Key Features

- **Zero-overhead transpilation**: TypeScript → Rust with no runtime dependency
- **Hot reload**: Development builds via cdylib for live reloading
- **Native interop**: Seamless integration with hand-written Rust via `native:` imports
- **Strict subset validation**: Rejects `any`, `class`, `try/catch`, `==`, etc.
- **Ownership inference**: Automatic `&T` / `&mut T` / `T` based on usage patterns
- **JSX support**: Transpiles to Ratatui builder patterns

## Installation

```bash
cargo install --path crates/rune-cli --features binary-rune
```

Or build from source:

```bash
cargo build --release -p rune-cli
```

## CLI Commands

```bash
# Development mode with hot reload
cargo rune dev

# Release build (static binary)
cargo rune build --release

# Type check only
cargo rune check

# Transpile a file to stdout
cargo rune transpile path/to/file.r.ts

# Initialize a new project
cargo rune init --name myproject
```

## Type Mapping

| Rune (TS) | Rust | Notes |
|---|---|---|
| `number` | `f64` | Default |
| `number` (literal) | `i32` | Integer literals |
| `string` | `String` | Heap allocated |
| `string` (literal) | `&str` | Borrowed |
| `boolean` | `bool` | |
| `T \| null` | `Option<T>` | |
| `{ok, value}` / `{ok, error}` | `Result<T, E>` | With `?` operator |

## The Rune Subset

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
├── rune.toml               # Rune configuration
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

1. `cargo rune dev` scans for `.r.ts` / `.r.tsx` files
2. SWC parses → validates subset → generates Rust to `target/rune-cache/`
3. Compiles to cdylib in `target/debug/`
4. Copies to `target/hot/libapp_<timestamp>.so`
5. Updates `target/hot/.current` symlink atomically
6. Host polls `.current`, unloads old dylib, loads new one
7. `AppState` survives in host heap

## Examples

See `examples/todox/` for a complete TODO application with:

- `.r.ts` logic files
- `.r.tsx` Ratatui views
- `.rs` native math functions
- Full hot reload support

## Architecture

```
.r.ts / .r.tsx  ──►  SWC Parser  ──►  Rune Analyzer  ──►  Rust Codegen  ──►  rustc
     │                    │                │                    │
     │                    │                │                    └── target/rune-cache/
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
