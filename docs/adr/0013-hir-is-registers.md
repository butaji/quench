# HIR is registers

The Wasm frontend converts the spec’s operand stack into typed registers while it validates. Common HIR is already a register program. The interpreter does not simulate a Wasm operand stack.

**Considered Options**: HIR stays stack-shaped and the runtime converts; the interpreter is a stack machine.
