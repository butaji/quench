# Shortest path to ADR 0005

ADR 0005 is authoritative. Each slice replaces one complete semantic family,
then deletes its predecessor; no permanent compatibility path is allowed.

1. Build `ProgramDb` (`Proven` / `Guarded` / `Unknown`) and the five OXC
   reducer contexts. Emit only generic residual ops; drop OXC arenas.
2. Make `ops!` the sole operation declaration and run its residual interpreter.
   Move lexical bindings, expressions, statements, and calls family by family;
   delete the handwritten AST, lowerer, and tree-walker as they become unused.
3. Replace runtime storage with tagged `Value`, `HeapRef(u32)`, declared heap
   layouts, shapes, slots, and slot/capture frames. No hot-path host pointers
   or string-keyed maps remain.
4. Add the shared completion/continuation model and declared static
   primordials/builtin metadata; retain readable Rust builtin algorithms.
5. Add only measured guarded specializations, quickening, and superinstructions.
   A baseline compiler is deferred and consumes these exact ops if ever added.

Facts may not suppress or reorder Proxy, accessors, coercion,
`Symbol.toPrimitive`, prototype mutation, direct `eval`, realms, or completion
ordering. Run the current test262 stage, formatter, and clippy for every slice;
the stage run is the sole conformance authority.
