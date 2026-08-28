# ADR 0003: Stackless VM and bounded isolate heap

Guest execution is an isolate-owned machine with explicit frames,
continuations, and one completion model; guest depth does not consume the Rust
stack. Calls, eval, generators, async suspension, exceptions, proxy traps, and
host boundaries use those transitions rather than a second interpreter.

Charge guest heap, code, frames, queues, and owned host resources to one
isolate budget. Allocation failure is an observable isolate failure/reset, not
a process panic. Canonical stores own identity, descriptors, prototypes,
environments, and resources; compact IDs are physical layout, not duplicate
semantics. Each transition is continue, jump, call, return, throw, suspend,
yield, or host effect; invalid state takes a cold, testable failure path.
