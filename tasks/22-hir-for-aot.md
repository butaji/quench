# Task 22: Stabilize the HIR for runtime and future AOT

## Status: COMPLETED

### What was done (2026-06-30)

#### Phase 1: serde support (unconditional)

Added `#[derive(serde::Serialize, serde::Deserialize)]` to all HIR types in `crates/quench-runtime/src/ast.rs`:

- `Span` — source position (byte offsets)
- `SourceLocation` — line/column location
- `Statement` — all statement variants
- `Expression` — all expression variants
- `BinaryOp`, `UnaryOp`, `CompoundOp`, `UpdateOp` — operators
- `VarKind` — var/let/const
- `ArrowBody` — expression or block
- `PropertyKey` — identifier, string, number, computed
- `ObjectProperty` — normal, getter, setter, spread
- `ArrayElement` — expression or spread
- `BindingElement` — identifier, array pattern, object pattern
- `ClassMember` — constructor, method, getter, setter, field
- `ClassBody` — container
- `ForBinding` — identifier, array pattern, object pattern
- `ForInit` — expression or var declaration
- `Program` — script

Added `serde` as an unconditional dependency in `Cargo.toml` (compile-time only, no runtime cost).

#### Phase 2: Reactive HIR nodes

Added new variants to the `Expression` enum for reactive primitives:

```rust
// Reactive HIR nodes (for signal-based reactive programming)
Signal { id: String, initial: Box<Expression> },
SignalGet { signal_id: String },
SignalSet { signal_id: String, value: Box<Expression> },
Memo { id: String, deps: Vec<Expression>, compute: Box<Expression> },
Effect { id: String, deps: Vec<Expression>, callback: Box<Expression> },
Render { id: String, component: Box<Expression>, props: Box<Expression> },
```

#### Phase 3: Interpreter updates

Updated `crates/quench-runtime/src/interpreter/eval_expr/main.rs` to handle the new variants. They currently return an error indicating they must be lowered by the reactive engine (Task 24). The minimal stub ensures the match is exhaustive.

#### Phase 4: Unit tests

Added 4 serde round-trip tests to `crates/quench-runtime/tests/runtime_tests.rs`:
- `test_hir_serde_roundtrip_simple` — Program with binary expression
- `test_hir_serde_reactive_nodes` — Signal, SignalGet, SignalSet
- `test_hir_serde_memo_effect_nodes` — Memo and Effect
- `test_hir_serde_class_body` — ClassBody with members

### Files changed

- `crates/quench-runtime/src/ast.rs` — serde derives + reactive HIR nodes
- `crates/quench-runtime/src/interpreter/eval_expr/main.rs` — new variant handlers
- `crates/quench-runtime/Cargo.toml` — added serde dependency
- `crates/quench-runtime/tests/runtime_tests.rs` — 4 new tests

### Remaining work (deferred to Task 24)

- Hook detection in the lowerer — emit `Signal`/`Memo`/`Effect` nodes when `ink.useState`, `ink.useMemo`, `ink.useEffect` are called
- Reactive engine — build dependency graph, propagate changes
- ANF transformation — flatten nested expressions into `Let` bindings during lowering
- `#[serde(tag = "type")]` or adjacent tagging for more compact serialization

### Verification

```bash
cargo check -p quench-runtime  # ✓ compiles
cargo test -p quench-runtime  # ✓ 86 passed, 2 ignored
cargo test test_hir_serde     # ✓ 4 new tests pass
```
