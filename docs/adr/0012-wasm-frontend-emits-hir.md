# Format crate vs VM crate

`quench-wasm` owns Wasm format handling and validation. `quench-runtime` owns
the shared HIR/MIR, instantiation, execution, arena, and GC. Validated input
crosses that boundary once; the format crate does not provide a second
executor.
