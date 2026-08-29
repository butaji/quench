# QuickJS is the JS layer on a Wasm VM

quench-runtime is a Wasm VM first: Native | Fast | Dynamic and Arena | GC match the Wasm store 1:1. QuickJS is the slower JS layer on top of that VM, not the Wasm GC heap.

**Considered Options**: fold QuickJS RC into Wasm `Storage::Gc`; keep wrapping `value::Value`; copy QuickJS as a JS layer.

**Decision**: Wasm store GC is structs/arrays/exns. QuickJS (`dynamic::Runtime`) owns JS objects, atoms, shapes, and RC+cycle.

| QuickJS fact | JS layer |
|---|---|
| `JSRuntime` owns heap, atoms, shapes, GC | `dynamic::Runtime` |
| `JSContext` is a realm with a global | `dynamic::Context` |
| `JSValue` tags; INT vs FLOAT64; RC iff tag < 0 | `dynamic::JsValue` / `Tag` |
| Atoms are `u32`; high half is immediate ints | `dynamic::Atom` |
| Shared shapes (proto + names + flags) | `dynamic::Shape` |
| RC + cycle pass, no explicit C roots | `Runtime::dup` / `free` / `run_gc` |
| Stack bytecode, max stack at compile time | `dynamic::Op` / `Bytecode` |
| Direct bytecode, no parse-tree IR for JS | JS frontend; wasm still emits register HIR |

Wasm Arena is linear memory and unboxed locals. Wasm GC is the store heap (shared across instances). `value::Value` + `HeapArena` root lists are the stale JS dual path.

Crossings stay Guard and Box. JS values climb: Dynamic `JSValue` → Fast guarded i32/number → Native unboxed.
