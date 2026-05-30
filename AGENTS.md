**runts** — a TypeScript compiler that outputs native Rust binaries instead of JS.

**The pitch:** TypeScript’s type system is powerful, but V8 erases it all and you pay 3–10× speed cost for GC and JIT. runts keeps the types: interfaces become structs, string unions become enums, generics monomorphize, async/await becomes tokio futures. Result: native binaries, <10 ms cold start, ~2 MB idle RAM, zero GC pauses.

**How it works:** TypeScript/TSX → oxc_parser (typed AST) → HIR → type-directed lowering → Rust source → `cargo build --release`. Dev mode skips Rust compilation entirely and runs an HIR interpreter with <50 ms hot-reload.

Make sure everything is covered with tests.

Commit each task, dont push.