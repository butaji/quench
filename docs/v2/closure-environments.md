# Lazy closure-environment promotion

The compiler conservatively marks a function `captures_locals` when its body
contains `MakeClosure`. That fact says an environment *may* be needed; it does
not prove the closure-producing path executes on every call.

Calls therefore begin with locals in the reusable frame vector even for a
marked function. Its compiled `LoadEnvLocal` and `StoreEnvLocal` operations
select frame locals while `frame.captured == false`. Immediately before an
executed `MakeClosure`, one explicit effect-edge operation moves the complete
locals vector into a traced `Cell::Environment`, links it to the parent, and
flips the frame state to captured. Later local operations use environment
slots, and the new closure retains that environment normally.

This is a two-state representation transition:

`FrameLocals(parent)` -> `HeapEnvironment(parent, slots)`

It is source- and benchmark-independent. Calls whose closure branch is not
taken avoid allocation; any executed closure creation takes the conservative
heap path. Proving that an executed closure itself cannot escape is a separate
optimization and is intentionally out of scope because a borrowed stack
environment would require stronger lifetime machinery.

Correctness invariants:

- promotion happens before the closure reads the frame environment;
- moving, rather than cloning, locals gives every later read/write one
  canonical slot representation;
- captured environments remain ordinary tracing-GC cells and roots;
- exception handlers already dispatch on the same `captured` state.
