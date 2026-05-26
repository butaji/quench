You are a senior Rust and compiler engineer. Design and implement "runts" — a
CLI tool that provides full framework-level compatibility with
Fresh[](https://fresh.deno.dev/docs/concepts/architecture) and Preact using only
a well-defined efficient subset of TypeScript + TSX.

Strict constraints:

- Zero external JS runtimes (no V8, no Deno, no WebAssembly JS).
- All code compiles to native Rust (TS/TSX -> High Level AST -> Rust source via in-mem codegen → binary).
- Users write pure Fresh-style Preact TSX (islands, partial hydration, routes,
  middleware, components, hooks) and it must work with minimal changes.

Requirements:

1. Precisely define the supported TS/TSX subset that covers 95%+ of real
   Fresh/Preact usage (JSX/TSX, hooks, async components, signals if used,
   File-based routing, etc.). List clear exclusions.
2. Architecture: parser (TSX) → semantic analysis → Rust code generation that
   emulates Preact runtime (virtual DOM or fine-grained reactivity via Rust
   signals/leptos-style).
3. Dev mode: extremely fast hot-reload with custom TS/TSX-subset runtime.
4. Production: full static compilation to efficient native binary (use
   Axum/Tower for server).
5. Full islands architecture and partial hydration implemented in Rust.
6. Any of those can have some alternative / more efficient clean solution

Deliver:

- Supported subset specification
- Detailed transpilation and runtime strategy
- Roadmap for MVP → full Fresh coverage
- Performance targets and trade-offs

Prioritize correctness and Fresh compatibility first, then performance. Be
ruthless on keeping the TS subset minimal but sufficient.
