# ADR 0004: One compact executable stream

Compiler output freezes once into one immutable fixed-width instruction stream.
Rare payloads, constants, names, and nested bodies are addressed directly, not
rescanned in parallel trees. Opcode/operand contracts are generated from one
declaration; lowering releases the mutable compiler form.

Ranges and encodings are validated before execution. Each slow instruction has
one cold payload, nested bodies use ranges in the same store, and compaction
never changes operation semantics or creates another interpreter.
