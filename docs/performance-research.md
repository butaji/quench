> **Optimization roadmap for quench-runtime, ranked by impact.**

# Performance Research

## Prioritized choices for a pure AST interpreter

1. **NaN-boxed `Value` + inline storage**
   - Pack pointers, integers, and special values into a single `u64` so `Value` is `Copy` and register-sized.
   - Small strings (≤6 bytes on 64-bit) stored inline to avoid heap hits.
   - Use `NonNull` for heap object pointers to enable niche optimizations.
   - *Current gap:* `Value` is an enum with boxed/`Rc` variants.

2. **Object shapes (hidden classes) without bytecode**
   - Shape transitions for every newly added property.
   - Property lookup becomes `shape_id → offset` indexing instead of hashmap probing.
   - Inline 2–3 slots in the object header; most JS objects have <4 properties.
   - Dense arrays: contiguous `Vec<Value>` for indices `0..length`, sparse map only for outliers.
   - *Current gap:* objects use `HashMap<String, Value>`.

3. **Flattened lexical environments**
   - Use SWC's scope data to assign each local a static depth and slot index.
   - Runtime scopes become `Vec<Value>` instead of `HashMap<String, Value>`.
   - Closure capture borrows from an arena-allocated parent frame by index.
   - *Current gap:* environments are `Rc<RefCell<HashMap<...>>>` chains.

4. **AST execution: reduce pointer chasing**
   - Arena-allocate the HIR in a `bumpalo` arena.
   - Replace recursive `eval` with a trampoline loop and explicit `Vec<Frame>`.
   - Use direct `match` dispatch on HIR variants; avoid `Box<dyn Fn()>`/vtables.
   - *Current gap:* interpreter is recursive and allocates per call.

5. **Immutable string interning**
   - Global atom table: every identifier/property name becomes `Atom(u32)` at parse time.
   - Runtime string ops compare IDs, not byte content.
   - Ropes for concatenation, flattening eagerly when length < 32 bytes.
   - *Current gap:* strings are compared as `String`/`compact_str`.

6. **Manual call stack and operand stack**
   - `Vec<CallFrame>` with `stack_base`, `return_addr`, `this`.
   - Evaluate expressions into a contiguous `Vec<Value>` operand stack instead of returning `Value`.
   - *Current gap:* recursive interpreter uses the Rust call stack and returns `Value`.

7. **Inline caching (IC) at AST level**
   - Store `(ShapeId, offset)` on hot `MemberExpr` / `CallExpr` HIR nodes.
   - Monomorphic fast path; polymorphic fallback with a small 4-entry cache.
   - *Current gap:* no IC; every property lookup does hashmap traversal.

8. **Arena + reference-counting hybrid**
   - Per-frame `bumpalo` arenas for temporary values.
   - `Arc` only for values that escape; lightweight mark-sweep or weak refs for cycles.
   - *Current gap:* everything is `Rc<RefCell<Object>>` with interior mutability.

9. **Rust-specific micro-optimizations**
   - `#[repr(C)]` tagged union for `Value`.
   - Avoid `RefCell` on hot paths; prefer `&mut` arena access.
   - Specialized fast paths for `Number + Number` and `String + String` before falling back to generic ops.
   - *Current gap:* generic `Value` math/string operations everywhere.

10. **Zero-cost host function bridge**
    - Host functions accept `&mut [Value]` (slice into operand stack) and return `Value`.
    - No `Vec` allocation per host call.
    - *Current gap:* host calls allocate argument vectors.

## Immediate wins for quench-runtime

The highest-leverage, lowest-risk changes are:

1. NaN-box `Value`.
2. Intern property/identifier names with `lasso`.
3. Dense arrays with inline slots.
4. Slot-indexed locals.
5. Operand stack instead of recursive returns.

Object shapes and ICs come next, once the value model is stable.

## Crates to use

- `nanbox` or custom bit-packing for `Value`.
- `lasso` for atoms.
- `bumpalo` for arenas.
- `indexmap`/`rustc-hash` where maps are still needed.
- `regress` for regex, `num-bigint` for `BigInt`.
- `mimalloc` or `tikv-jemallocator` as global allocator.

## Future AOT/JIT

Use `cranelift-*` rather than LLVM/`inkwell`. The HIR should remain high-level enough to feed a backend without introducing a separate bytecode layer.
