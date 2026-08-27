# ADR 0004: One compact executable stream

## Principles

- Freeze compiler output into one immutable executable store.
- Use a compact fixed-width instruction stream for fetch and dispatch.
- Keep uncommon semantic payloads, constants, names, and nested ranges out of
  the hot fetch record and address them directly; never scan a parallel tree.
- Generate opcode and operand contracts from the shared operation declaration.
- Lower each source operation once, then release the mutable compiler form.
- Preserve complete operation semantics in existing handlers; compaction is a
  storage and dispatch decision, not an alternate interpreter.

## Invariants

- Every instruction has a valid opcode and operands for its declared contract.
- Every slow instruction has exactly one matching cold payload; fast
  instructions have none.
- Every code range is checked against the immutable store before execution.
- Nested bodies refer to ranges in the same store rather than duplicated code.
- Invalid encodings fail before execution through a deterministic error path.
