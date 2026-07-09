# Static Specialization for the Shadow Tree Interpreter

**Status:** Phase 1–3 implemented in `crates/quench-runtime/src/shadow.rs`. Phases 4–5 (direct calls/imports, array/string/loop fast paths) are future work.

## Goal
Exploit the static subset of JS/TS (ES modules, block scope, classes, literal property names, TS type annotations) to make the SSTI faster without a JIT. We front-load analysis at parse/link time and generate specialized `ShadowNode` variants that skip runtime checks, with generic fallbacks when reality disagrees.

## Phase 1 — Static Binding Resolution
- Add `Binding` enum: `Local(u16)`, `Upvalue(u16)`, `Global(Atom)`, `Import(u16)`, `ConstInt(i32)`, `ConstString(Atom)`.
- Walk SWC scope tree. If a function/module has no `eval`/`with`, mark it `ModuleMode::Static`.
- In static modules, replace identifier `ShadowNode::GlobalRead`/`LocalRead` with `BindingRead(Binding)`.
- Static mode uses a flat frame layout: `frame.locals` is a fixed `Vec<JSValue>`; no heap `Environment` objects per scope.

## Phase 2 — Type-Directed Node Specialization
- Attach `TypeHint` to shadow nodes: `Any`, `Int32`, `Double`, `Number`, `String`, `Object(shape_id)`.
- `Add` node gets variants:
  - `SpeculativeNumber` — assumes both operands are numbers (from TS or prior feedback), falls back to generic.
  - `Int32` — both operands observed int32.
  - `Generic` — full JS semantics.
- On deoptimization, mutate the node in-place via `Cell`.

## Phase 3 — Pre-Allocated Shapes
- For `class` declarations and object literals with literal keys, build the final shape at parse time.
- Add `StaticPropWrite { obj, shape_id, offset, value }` and `StaticPropRead { obj, shape_id, offset }` nodes.
- Constructor bodies write instance fields with no runtime property lookup.

## Phase 4 — Direct Calls and Module Imports
- Resolve ES module imports at link time.
- `Call` node gets `Direct { target, args }` variant when callee is a statically bound function.
- Deoptimize to `Dynamic` on reassignment.

## Phase 5 — Fast Array / String / Loop Nodes
- `FastArrayIndex` / `FastArrayVar` for known arrays + integer indices.
- `Int32Range` loop node for `for (let i = start; i < end; i++)`.
- String `.length` direct field read.

## Implementation Order
1. Binding resolution + `ModuleMode::Static` flag.
2. Type hints + `Add` specialization.
3. Pre-allocated shapes + static property access.
4. Direct calls + imports.
5. Array/string/loop fast paths.

## Verification
- Add unit tests in `shadow.rs` for:
  - static `LocalRead` vs `GlobalRead`
  - `Add` Int32 specialization and deoptimization
  - pre-allocated shape constructor write
  - `for (let i = 0; i < n; i++)` Int32Range loop
- `cargo test --workspace` must stay green.
