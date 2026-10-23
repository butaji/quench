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

# What a closure keeps alive

Every closure an activation makes shares that activation's environment, so the
environment holds every binding any of them captures. A full collection does
not trace a closure's environment edge whole. It traces only what the closure
can read (`heap/environment_trace.rs`), and clears every slot no reader reads.
Tracing and clearing both read `bytecode::ClosureReads`, which the compiler
derives once per program from each function's `LoadCapture`/`StoreCapture`
facts (`compile/capture_layout.rs`):

- at each distance out, the slots the closure's code reads, and those of every
  function nested in it, which the closure may still create;
- for a class constructor, the reads of its instance initializer, which runs
  on the constructor's environment;
- the slots the runtime finds by name (`local_name_read_by_name`): home objects
  and compiler bindings the environment declares;
- an arrow's lexical `this`, up to the first enclosing non-arrow, whose
  activation binds it.

A closure that resolves names dynamically (direct `eval`, `with`, binding
sites), or that is nested in or encloses such a function, reads its whole
chain. So does an environment past the closure's program (an eval's caller), a
program root, or a layout the table does not describe (`with` layers, eval
scopes, native slots). Every other edge to an environment reads it whole: a
frame, a generator, a binding reference, a module record. No reader ever reads
the value of a root projection slot (a global var or module import): reads go
through it to the binding, and stores check only that it is initialized. So
the trace keeps a projection's initialization state but not its value.

A cleared slot holds an uninitialized binding (`Value::DELETED`) the
environment owns. A cleared projection holds `undefined`. Minor collections, and
heaps without function records (which mark in parallel), trace environments
whole. `vm/closure_retention_tests.rs` asserts the contract under the
specialized and unspecialized compilations, before and after tier-up.
Measurements are in `evidence/closure-retention-linux-2026-10-23`.
