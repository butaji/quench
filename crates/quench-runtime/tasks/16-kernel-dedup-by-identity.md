# 16 — Shared-kernel deduplication by structural identity

Status: planned

`then_kernel` composes a stencil prefix with a `Rc<Kernel<Mid, Out>>` without copying the kernel's bytes into the caller's image. Today distinct functions with the same exit contract (for example, plain `ReturnState` epilogues) may still obtain distinct `Kernel` instances instead of the one shared mapping the category permits. Build a kernel registry keyed by `(level, Mid state, Out state, byte identity)` so every function whose exit shape matches reuses the same `Rc<Kernel<_, _>>` rather than materializing an equivalent epilogue again.

This is a direct consequence of composition-by-reference already proven in [[01-stencil-category-core]]: sharing is only correct because attaching a kernel is composition with a referenced object, not a copy. The task is to make the runtime actually exploit that guarantee everywhere a matching kernel exists, not only within one function's own compilation.

Acceptance: a counter distinguishes "kernel instances materialized" from "kernel instances referenced"; a program with N functions sharing an exit contract produces one kernel mapping, not N; existing category-law tests keep passing unmodified.
