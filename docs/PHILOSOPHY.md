# Runts Philosophy: Framework-Agnostic TS/TSX → Rust

## Core Principle

**Runts is a general-purpose compiler and runtime for a subset of TypeScript/TSX.**

Fresh, Deno, Hono, Express — these are **users** of runts, not part of it.
The compiler knows nothing about Fresh's island architecture or Hono's Context.
It compiles TypeScript/TSX source to native Rust binaries, period.

## What Runts Is

- A **compiler**: TS/TSX → HIR → Rust source → native binary
- A **runtime**: Dev server with hot-reload (rquickjs + Yoga) + production server (compiled native)
- A **language subset**: The parts of TS/TSX that map cleanly to Rust semantics

## What Runts Is NOT

- NOT a Fresh clone
- NOT a Hono wrapper
- NOT Deno-specific
- NOT tied to any web framework's conventions

## Architecture Implications

### 1. HIR is Pure TypeScript

The HIR represents TypeScript/TSX constructs, not framework concepts:

```rust
// GOOD: HIR represents a TS function export
Export::NamedValue { name: "handler", value: Expr::Object { ... } }

// BAD: HIR hardcodes Fresh route handler concept
Export::Handler { methods: vec!["GET", "POST"] }
```

Framework conventions (routes, islands, handlers) are detected by **analyzers** that run on the HIR, not baked into the HIR itself.

### 2. Analyzer is Pluggable

Different projects can have different analyzers:

```rust
/// Detects Fresh-style patterns (islands, routes, _app.tsx)
pub struct FreshAnalyzer;

/// Detects Hono-style patterns (c.json(), c.html(), handlers)
pub struct HonoAnalyzer;

/// Detects Express-style patterns (app.get(), app.post())
pub struct ExpressAnalyzer;

/// Generic analyzer — just exports what's in the file
pub struct GenericAnalyzer;
```

All produce the same HIR. The analyzer only affects:
- Route table generation
- Island bundle detection
- Dev server routing

### 3. Runtime is Minimal

The runtime provides:
- HTTP server (axum)
- Static file serving
- rquickjs dev engine (TSX → JS → QuickJS + Yoga bridge)
- Hot-reload file watcher

It does NOT provide:
- Fresh's island hydration system
- Hono's middleware chain
- Express's router

These are compiled from user TS/TSX source.

### 4. Codegen is Framework-Agnostic

The Rust codegen emits idiomatic Rust from HIR:

```typescript
// User source
export const handler = {
  async GET(req: Request, c: Context) {
    return c.json({ hello: "world" });
  }
};
```

```rust
// Generated Rust (Hono pattern detected by analyzer)
pub const handler = Handler {
    get: Some(|req: Request, c: Context| async move {
        Response::builder()
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&json!({"hello": "world"})).unwrap()))
            .unwrap()
    }),
    ..Handler::default()
};
```

The `c.json()` call is just a method call in the HIR. The codegen recognizes it and emits the appropriate Rust. If the user had written `return new Response(JSON.stringify({...}), { headers: {...} })`, the codegen would emit that pattern instead.

## Supported TS/TSX Subset

### Statements
- Variable declarations (`const`, `let`, `var`)
- Function declarations (named, async, generator)
- Arrow functions (expression and block body)
- Return, If/Else, While, Do/While
- For, For-In, For-Of
- Switch, Try/Catch/Finally
- Throw, Break, Continue
- Labeled statements

### Expressions
- All literals (string, number, boolean, null, undefined, bigint, regexp)
- Template literals
- Binary, Unary, Update, Logical, Conditional operators
- Member access (computed and static, optional chaining)
- Function calls and `new`
- Object literals (properties, methods, getters, setters, shorthand, spread)
- Array literals (elements, spread, elision)
- Assignment operators
- Arrow functions and function expressions
- Await and Yield
- Spread operator
- Sequence expressions
- Meta properties (`new.target`, `import.meta`)

### JSX
- Elements (`<div>`, `<span>`)
- Components (`<Header />`)
- Fragments (`<>...</>`)
- Attributes (string, expression, boolean, spread)
- Event handlers (`onClick={...}`)
- Children (text, expression, nested JSX, fragments)
- Spread children
- Dynamic components (`{Component}`)
- Member expression components (`<Component.Sub />`)

### Types
- Primitives: `string`, `number`, `boolean`, `null`, `undefined`, `void`, `any`, `unknown`, `never`, `bigint`, `symbol`
- Arrays: `T[]`, `Array<T>`
- Tuples: `[T, U]`
- Unions: `T | U`
- Intersections: `T & U`
- Objects: `{ key: T }`
- Functions: `(a: A) => B`
- Type references: `T<U>`
- Type aliases: `type T = U`
- Interfaces: `interface T { ... }`
- Generics with constraints and defaults
- Index types: `T[K]`
- Conditional types: `T extends U ? V : W`
- Mapped types: `{ [K in T]: V }`
- Template literal types: `` `hello ${T}` ``
- `keyof`, `typeof`, `infer`, `readonly`
- Literal types: `"foo"`, `42`, `true`

### NOT Supported (by design)
- Classes (use functions + closures)
- `eval()`
- `with` statement
- `arguments` object
- Prototype manipulation
- Dynamic property access with arbitrary expressions (static analysis required)
- Some advanced TypeScript features: `satisfies`, namespace merging, enum merging

## Project Conventions (User-Level)

Users adopt conventions by placing files in specific directories:

```
my-project/
  routes/           → HTTP route handlers (convention, not framework)
  islands/          → Client-interactive components (convention)
  components/       → Shared components
  static/           → Static assets
  _app.tsx          → App wrapper (convention)
  _layout.tsx       → Layout wrapper (convention)
```

The **analyzer** detects these conventions. The **compiler** just compiles TS/TSX.

## Runtime Modes

### Dev Mode
```
TS/TSX source → oxc_parser → oxc_codegen → JS bundle → rquickjs + Yoga bridge → render
                    ↑                                              ↑
                    └──────────── File watcher ────────────────────┘ (hot-reload)
```

No Rust compilation. Changes reflected in <100ms.

### Build Mode
```
TS/TSX source → oxc_parser → HIR → Rust Codegen → cargo build → native binary
```

Single `runts build` command. Output is a native binary in `.runts/build/target/release/`.

## Summary

| Concern | Framework (Fresh/Hono/etc) | Runts |
|---|---|---|
| Language | TypeScript/TSX | ✅ Compiler |
| Routing | `routes/` convention | ✅ Analyzer detects, codegen emits |
| Islands | Client hydration | ✅ Analyzer detects, codegen emits bundles |
| Middleware | `c.next()`, `app.use()` | ✅ Compiled from user source |
| JSX | `<Component />` | ✅ Transformed to Rust VNode/ |
| Runtime | Fresh server, Hono app | ✅ Generic axum-based runtime |
| Dev Server | `deno task start` | ✅ `runts dev` |
| Production | Deploy to Deno Deploy | ✅ Native binary |

Runts is the **compiler and runtime**. Everything else is user code.
