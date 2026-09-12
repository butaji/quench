# 81 — Per-opcode profiling symbols

Status: planned

Each `deegen_dyn_*` handler in `stencil-aot/handlers.rs` is already its own
`extern "C" fn` rather than being folded into one dispatch loop, which in principle
lets a sampling profiler (`perf`, Instruments) attribute time to individual opcodes —
the same property CPython's team relied on after their tail-call interpreter rewrite to
find and fix specific slow bytecodes.

Audit which handlers currently carry `#[inline(always)]` (present on several small
helpers at lines 194, 199, 204, 213) versus which are left as standalone symbols. Any
dispatch-level handler marked `#[inline(always)]` will not appear as a distinct symbol
in release builds, defeating per-opcode attribution. Establish (and document) a naming
convention or attribute (e.g. `#[no_mangle]` or a lint) that guarantees every
dispatch-level `deegen_dyn_*` handler retains a distinguishable symbol in release
builds, while leaving small leaf helpers free to inline.

Acceptance: a release build's symbol table contains one distinct, demangleable symbol
per dispatch-level opcode handler; a sample `perf record`/`perf report` run against one
of the existing v8v7 suites attributes samples to individual opcodes rather than to a
single aggregate function; no functional or performance change expected other than to
the symbol table.

Related: 36 (direct opcode stencils), 05 (performance harness).
