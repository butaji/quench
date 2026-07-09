# Self-Optimizing Shadow Tree Interpreter (SSTI) Design

## Goal
Build a parallel, non-recursive execution path for the Quench runtime that uses an explicit value/call stack, NaN-boxed values, shape-based objects, and self-optimizing AST nodes.

## Scope
First milestone: a minimal but real SSTI that can evaluate arithmetic, locals, property reads, and simple object construction without growing the Rust stack. It lives alongside the existing interpreter; existing tests must stay green.

## Architecture

```
SWC AST  ->  ShadowNode tree  ->  ShadowVm::run()
                  ^
                  +-- nodes mutate their own cache/variant at runtime
```

### Components

1. `nanbox.rs`
   - `JSValue`: `#[repr(transparent)] u64`.
   - Tags: `double`, `int32`, `object`, `string`, `symbol`, `null`, `undefined`, `true`, `false`, `hole`.
   - Conversion helpers and inline accessors.

2. `shadow.rs`
   - `ShadowNode<'a>` enum in a `bumpalo` arena: `Add`, `PropRead`, `LiteralInt`, `LiteralDouble`, `LiteralString`, `LocalRead`, `GlobalRead`, `This`, `Block`, `Return`, `Call`, `NewObject`, `StoreProp`.
   - `ValueStack` (flat `Vec<JSValue>`) and `Frame` stack.
   - `ShadowVm<'a>` with an iterative work stack (`Continuation`) instead of recursive `eval`.
   - `Add` self-optimizes: `Uninit` -> `Int32` | `Double` | `StringConcat` | `Generic`.
   - `PropRead` self-optimizes via shape cache: `Uninit` -> `Monomorphic` -> `Megamorphic`.
   - Uses the existing `ShapeInterner` and `Arena<Object>` in `Context`.

3. `Context` integration
   - `Context::eval_shadow(source: &str, mode: shadow::ModuleMode) -> Result<Value, JsError>`.
   - Parses with SWC, resolves bindings in `ModuleMode::Static`, lowers a *minimal* subset to ShadowNode.
   - Returns legacy `Value` so callers do not change.

## Design Decisions
- **Switch dispatch**: `match` on `ShadowNode` tag, not visitor trait.
- **Iterative execution**: explicit `Continuation` work stack prevents native stack overflow.
- **Arena allocation**: `bumpalo::Bump` owns all nodes for the compilation unit.
- **NaN boxing**: single 64-bit value, fast tag checks.
- **Shapes**: reuse the existing `ShapeInterner`; object layout is inline slots + out-of-line vector.
- **No full JS semantics yet**: defer getters/setters, prototypes, exceptions, and full builtins to later tasks.

## Testing
- Unit tests in `nanbox.rs` for value encoding/decoding.
- Unit tests in `shadow.rs` for stack machine and self-optimization.
- Integration test in `lib.rs` via `eval_shadow(..., ModuleMode::Static)` for `1 + 2`, `var x = 1; x + x`, and `var o = {a: 5}; o.a`.
- `cargo test --workspace` must pass before declaring done.
