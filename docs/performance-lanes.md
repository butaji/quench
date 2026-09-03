# Performance lanes

Control runs use an explicit optimized artifact and report repeated raw
per-fixture results. Diagnostic builds may trace, sample, or count the same VM,
but never supply score numbers. The current performance target is the ordinary
fact-generated interpreter with catalog-backed opcode handlers, bounded
quickening, and reusable generic fast paths, plus the bounded copy-and-patch
region-stencil tier described in
[`copy-and-patch-jit.md`](copy-and-patch-jit.md) — the sole exception to an
otherwise interpreter-only scope, gated behind tasks 011/016/019/026. Join
evidence by artifact/profile/run identity; unavailable data is unknown, not
zero. The runtime contains no workload-specific kernel feature; reusable
fact-guarded kernels remain valid runtime machinery.
Measurement failures are fixed in the measurement or general runtime, never
with workload-shaped behavior. The implementation queue is
[`tasks/index.json`](../tasks/index.json). Reproducible commands and the
interpretation rules for their output are indexed in
[`architecture-evidence.md`](architecture-evidence.md). The VM declaration remains the
single `vm_op!` macro catalog; generated mechanics may change as facts grow,
but semantic handlers stay explicit and benchmark-independent.
