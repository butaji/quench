# HIR is registers

The Wasm frontend converts the specification operand stack into typed
registers while validating. Common HIR is register-based, and the interpreter
does not simulate a Wasm operand stack.
