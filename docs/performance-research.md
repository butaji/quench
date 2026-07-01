> **Optimization roadmap for quench-runtime.**

# Performance Research

## Interpreter-level wins (current priority)

1. **NaN-boxed `Value`** — pack primitives/pointers/tags into a single `u64`.
2. **String interning** — use `lasso` for property names and identifiers.
3. **Object shapes + inline caches** — cache `(shape_id, offset)` on hot property/call sites.
4. **Slot-indexed environments** — assign locals stack slots instead of `HashMap` lookups.
5. **Arena allocation** — `bumpalo` for frames and temporary objects.
6. **Fast maps** — `indexmap`/`rustc-hash` for ordered/integer-keyed maps.
7. **Regex** — `regress`; **BigInt** — `num-bigint`.

## Future AOT/JIT

Use `cranelift-*` rather than LLVM/`inkwell` for native code generation. The HIR is intended to be consumable by a future backend without re-parsing source.

## Crates to evaluate

`lasso`, `indexmap`, `rustc-hash`, `bumpalo`, `regress`, `num-bigint`, `mimalloc`, `cranelift-*`.
