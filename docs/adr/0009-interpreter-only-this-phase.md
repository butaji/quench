# Interpreter only in this phase

The VM executes specialized MIR by interpretation. No JIT and no AOT until a later decision. Native kernels inside the interpreter (for example a memory fill) are allowed; they are not a compiler.

**Considered Options**: interpreter first with JIT later as a second consumer of the same MIR; native/JIT from day one.
