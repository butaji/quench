# 305 — Explicit host root scopes for non-owning values

Status: complete

The object tag is a one-word, non-owning `ObjectHandle`; retaining an ordinary `Value`
outside the active guest-frame graph must therefore not silently imply ownership. Model the
host boundary as an explicit, stack-disciplined root scope. Entering a scope appends values
to one VM-owned root sequence, collection traces that same sequence, and leaving the scope
truncates it back to the captured base. Internal register/local moves remain plain word
copies and pay no reference-count traffic.

This is the same small-kernel/data-first rule as the stencil pipeline: the one fact is the
ordered root sequence, and scope entry, tracing, and release are derived views of it. Nested
scopes compose by sequence concatenation and restore by prefix identity; no object-specific
branch is added to the hot value-copy path.

Acceptance: a host-retained function keeps its object-heap prototype alive across an
explicit collection, releasing the scope makes that otherwise unreachable cycle
collectable, the constructor regression passes with collection at every completed frame,
and normal plus stress-GC release suites pass.

## 2026-09-10 implementation

`Vm::retain_host_roots` captures a typed `HostRootScope` base and appends its fixed-size
input array to `Vm::host_roots`; `Vm::release_host_roots` truncates to that base. The precise
collector traces the sequence with the other canonical root sources. This makes ownership
at the host/VM edge explicit without restoring `Rc` operations to object values.

Bump-first allocation exposed the missing boundary because a reclaimed prototype address
was no longer immediately reused: a Rust-held `FunctionValue` then reached a genuinely
`Free` prototype during marking. The constructor test now opens a host root scope for the
three values retained across VM entries. A focused lifecycle test proves both directions:
the function prototype remains `Allocated` while scoped and becomes `Free` after release.
The full normal and every-frame stress suites passed 108 of 108 before the focused test was
added; the focused test itself passes in both modes. Task 299's rejected candidate passed a
109-test matrix while its allocator-specific test was present. After that experiment was
removed, the final host-root source has 108 tests and is validated independently.
