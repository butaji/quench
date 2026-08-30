# WebAssembly rules

Quench has one runtime and language-specific frontends. The Wasm frontend owns
decoding, validation, and conversion to the common register HIR. The runtime
owns instantiation, execution, memory, tables, exceptions, and host calls.

## Rules

- Wasm enters the Native layer; JavaScript and Typed TypeScript use the same
  Native | Fast | Dynamic ladder where their facts permit.
- A guard or a box is the only layer transition. Do not create per-language
  object models or a second executor.
- HIR is typed and register-based. The interpreter executes specialised MIR;
  it does not simulate a Wasm operand stack.
- Third-party decoding, validation, and wast parsing are allowed. Third-party
  execution and a guest Wasm interpreter are not.
- Native values remain unboxed where the representation is known. Dynamic
  operations remain available for values whose meaning is unknown.
- Use one tracing GC for long-lived heap objects and an arena only for bounded
  scratch. Instance memory and tables have instance lifetime; table references
  are GC roots.
- Traps, tagged Wasm exceptions, and Dynamic throws share one unwind walk but
  retain distinct matching rules.
- The spectest harness is a host adapter over the common call ABI. Node's
  `WebAssembly` API is a separate compatibility surface.
- Compatibility is measured by every directive in the vendored spec suite,
  including proposals. Do not use a skip list or file-only scoring.
- Tests assert observable validity, linking, instantiation, results, traps,
  exceptions, exhaustion, and host effects—not internal HIR or register shape.
