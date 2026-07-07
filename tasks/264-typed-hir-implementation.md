# Task 264: Implement the typed HIR execution model

## Status: PENDING

## Goal

Replace the recursive AST interpreter with a type-aware HIR + trampoline interpreter that is aligned with JS/TS/JSX/TSX runtime semantics and uses TypeScript types for performance. Unimplemented features must throw a clear error; stubs and silent fallbacks are forbidden.

## Implementation strategy — quick wins first

### Phase 0: Runtime correctness quick wins
Complete before touching HIR infrastructure. These remove silent failures and give an honest baseline.

- Task 250 — Preserve thrown values in try/catch/throw
- Task 253 — Load real test262 harness includes
- Task 91 — Audit and shrink test262 feature skip list
- Task 97 — Improve negative-test matching by type/phase
- Task 147 — Fix Array.prototype.flat() prototype chain
- Task 191 — Fix toLocaleLowerCase/toLocaleUpperCase missing dispatch
- Task 132 — Fix Error constructor name property
- Task 239 — Fix Boolean/String prototype registration

**Verification:** `cargo test -p quench-runtime` shows no new silent failures.

### Phase 1: Value model + shapes
Introduce the simplified runtime object model without changing the evaluator yet.

- `Object` with shape-indexed property storage
- `Shape` interner per realm
- Unified `FunctionObject` (user + native + bound)
- `StringInterner` for property names and symbols

**Rule:** Any unsupported shape/lookup path must panic/throw, not silently fall back to a HashMap.

### Phase 2: Trampoline call stack
Replace native-stack recursion with explicit `CallFrame`s.

- `CallFrame` with locals, upvalues, pc, return target
- Trampoline dispatch loop
- Frame allocation and argument binding
- Proper upvalue capture with shared cells

**Rule:** Unsupported call modes panic; no stub frames.

### Phase 3: Untyped HIR builder
Build a parallel HIR lowering pass from the source AST.

- Basic blocks, jumps, terminators
- Resolved bindings (local/global/upvalue/this/import)
- Lowered destructuring, rest, default params
- Lowered optional chaining / short-circuit logic
- Exception tables

**Rule:** Every AST node that cannot be lowered must panic in the builder with a clear message.

### Phase 4: Type extractor + typed HIR
Collect TypeScript annotations and inferred types and attach `Ty` metadata.

- Type lattice: primitives, objects with shapes, arrays, functions, unions
- Type-specialized ops with runtime guards
- Unboxed locals for stable primitive types
- Shape-driven object layout from interfaces/classes

**Rule:** Specializations are guarded; fallback generic ops must be fully implemented.

### Phase 5: Switch to HIR interpreter
Run the full test suite through the typed HIR interpreter.

- Interpret HIR directly in the trampoline loop
- Every unsupported HIR op throws until implemented
- Compare against the recursive interpreter baseline

### Phase 6: Retire recursive interpreter
Delete the old recursive evaluator once the HIR interpreter matches it.

## Verification

```bash
cargo check
timeout 120 cargo test -p quench-runtime
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
```

## Dependencies

- Tasks 250, 253, 91, 97, and the small spec fixes above
- Task 85 (trampoline interpreter) overlaps with Phase 2
- Task 88 (Rust runtime leverage) supports Phases 1–2
- Task 82 (whole-suite conformance analysis) is unblocked after Phase 5

## Targets

- **Suite:** `both`
- **Batch:** 1
- **Target subset:** n/a (HIR execution infrastructure)
- **Blocked by:** 85, 88
- **Exit criteria:** HIR interpreter passes the same tests as the recursive interpreter and the recursive interpreter is retired.
