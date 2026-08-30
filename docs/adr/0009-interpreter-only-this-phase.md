# Interpreter only in this phase

The VM executes specialised MIR by interpretation. Native kernels are allowed
inside that executor, but JIT and AOT are separate future decisions and must
not introduce alternate semantics.
