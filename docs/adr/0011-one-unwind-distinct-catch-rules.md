# One unwind, distinct catch rules

Traps, Wasm tagged exceptions, and Dynamic throws share one frame walk. Catch rules stay distinct: a trap is not a `try_table` exception and is not a JS `throw`; a tagged Wasm exception matches Native `try_table`; a Dynamic throw matches Dynamic `catch`. At a layer boundary an uncaught Native trap or exception becomes a Dynamic error.

**Considered Options**: traps abort the invocation without the shared walk; Native failures never convert at the JS boundary.
