# Wasm frontend emits HIR

`quench-wasm` parses, validates, and translates Wasm into the runtime’s common HIR. `quench-runtime` owns HIR, specialises HIR to MIR, and interprets. The Wasm crate does not execute; the runtime does not decode Wasm bytes.

**Considered Options**: runtime performs Wasm→HIR; frontend emits MIR and skips HIR.
