# Dynamic layer copies QuickJS architecture

The Dynamic layer is QuickJS's engine shape, not a second JS object type beside Native/Fast.

**Considered Options**: keep wrapping the existing `Value` enum; invent a new tagged word; copy QuickJS's facts.

**Decision**: copy QuickJS's major decisions into `quench-runtime` Dynamic, keep the Native | Fast | Dynamic ladder.

| QuickJS fact | Quench |
|---|---|
| `JSRuntime` owns heap, atoms, shapes, GC | `dynamic::Runtime` |
| `JSContext` is a realm with a global | `dynamic::Context` |
| `JSValue` tags; INT vs FLOAT64; RC iff tag < 0 | `dynamic::JsValue` / `Tag` |
| Atoms are `u32`; high half is immediate ints | `dynamic::Atom` |
| Shared shapes (proto + names + flags) | `dynamic::Shape` |
| RC + cycle pass, no explicit C roots | `Runtime::dup` / `free` / `run_gc` |
| Stack bytecode, max stack at compile time | `dynamic::Op` / `Bytecode` |
| Direct bytecode, no parse-tree IR for JS | Dynamic frontend; wasm still emits register HIR |
| Interpreter only | ADR 0009 |
| Numbers: i32 fast path or f64 | Native = unboxed; Fast = guarded; Dynamic = tagged |

Storage is **Arena | GC**, derived from layer: Native is Arena (linear memory, instance heap, bytecode, reset as a region). Fast/Dynamic are GC (QuickJS RC + cycle pass, no explicit roots). `value::Value` + `HeapArena` root lists are the stale JS dual path; Dynamic does not wrap them.

Crossings stay Guard and Box. Wasm stays Native register HIR. JS values climb: Dynamic `JSValue` → Fast guarded i32/number → Native unboxed.
