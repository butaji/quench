# Quench vocabulary

Quench uses one runtime with three frontends: Typed TypeScript, JavaScript,
and WebAssembly. The runtime owns execution, allocation, strings, functions,
exceptions, modules, host calls, and profiling.

- **Native** is fully known, unboxed execution: Wasm scalars, fixed layouts,
  linear-memory bytes, and direct calls.
- **Fast** is specialised execution guarded by facts such as shape, slot, or
  number kind.
- **Dynamic** resolves meaning at run time for operations such as JavaScript
  coercion, computed properties, and unknown calls.
- A **layer** describes representation and dispatch, not a language. Guards
  and boxes make layer changes explicit.
- **HIR** is the shared typed register program emitted by each frontend.
  **MIR** is its specialised executable form.
- **GC** owns heap data whose lifetime exceeds a call, compile, or job.
  **Arena** storage is limited to bounded scratch and is never a long-lived
  instance resource.
- A **trap** is a Native instruction failure. A **Wasm exception** is a tagged
  throw matched by a Wasm handler. Dynamic throws follow JavaScript catch
  rules; all three use the shared unwind mechanism.
- The **spec suite** is the vendored WebAssembly testsuite. Compatibility is
  measured across every directive, including proposal tests.

Keep these concepts shared across frontends. Do not introduce language-owned
object models, duplicate collectors, or alternate execution semantics.
