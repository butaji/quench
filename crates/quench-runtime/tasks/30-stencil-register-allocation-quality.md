# 30 — Register allocation quality within stencils

Status: planned

Improve the register allocation used inside individually AOT-extracted stencil templates ([[02-rustc-aot-templates]]), following Cranelift's regalloc2 and AsmJit's approach of fast, decent allocation over optimal allocation. This is orthogonal to the categorical composition structure — it is an implementation-quality concern of a single stencil's own codegen, not a composition-level optimization — and should not block or depend on any other item in this file.

Acceptance: measured reduction in spill/reload traffic within hot single-stencil templates on the existing benchmark harness ([[05-performance-harness]]); no change to stencil connector contracts or composition behavior.
