# ADR 0003: Stackless VM and isolate-bounded heap

## Principles

- Execute guest JavaScript with an explicit isolate-owned machine, activation
  stack, and control frames. Guest call depth must not grow the Rust stack.
- Represent execution as state transitions over immutable code and explicit
  continuations. Calls, constructors, `eval`, generators, async suspension,
  exceptions, and proxy traps use the same transition machinery.
- Keep one semantic completion model. Host boundaries translate transitions;
  they do not create a second interpreter or silently discard suspension.
- Bound JavaScript-owned memory at the isolate boundary. Charge heap objects,
  code, continuations, queues, and host-owned resources to one budget.
- Allocation failure is an observable isolate error or isolate reset, never an
  unchecked process panic.
- Keep identity, descriptors, prototypes, environments, and resource state in
  their canonical stores. Do not add shadow stacks, duplicate heaps, or
  subsystem-specific universes.
- Use compact IDs and references where they preserve semantics; physical
  layout is an implementation detail, not a second semantic representation.
- Native execution, if admitted, consumes the same residual operations and
  falls back to the ordinary VM for unknown or invalid states.

## Required invariants

- Every transition has one explicit outcome: continue, jump, call, return,
  throw, suspend, yield, or host effect.
- Invalid opcodes, ranges, references, and frame states fail through a cold,
  testable path.
- Slow semantics remain complete before any guarded fast path is introduced.
- Tests cover deep calls, nested control flow, async resumption, exceptions,
  allocation limits, and observable host effects against a Node oracle.
