# Format crate vs VM crate

`quench-wasm` is the Wasm **format**: parse, validate, wast scoring. `quench-runtime` is the **VM**: load bytes into the store, instantiate, interpret Native | Fast | Dynamic with Arena | GC.

**Considered Options**: frontend lowers to HIR; runtime never sees Wasm bytes.

**Decision**: the VM loads validated binaries (`wasm::load`). The format crate does not own execute or a second lowering compiler.
