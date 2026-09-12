# 43 — Codegen-quality audit of the rustc/LLVM extraction pipeline

Status: in_progress

The rejected [[32-element-kind-guarded-arrays]] generic-run experiment uncovered a
catalog correctness hole: `deegen_dyn_run` contained an unpatched local jump-table
relocation and an unpatched `_fmod` call, causing `Illegal instruction` after copying.
[[102-reject-unrepresented-aot-relocations]] now disables jump tables in cooked
handlers and rejects every relocation that is not an explicit continuation hole.

Post-[[103-owned-slot-overwrite]] samples are stored under
`reports/native-samples-after-owned-slots/`. They still showed `Value` drop glue and
`DynJitCode::run` at function boundaries; [[106-cleared-slot-pool-invariant]] converted
that evidence into a +7.49% accepted full-suite improvement.

Distinct from [[30-stencil-register-allocation-quality]] (register allocation within a single stencil's own logic): audit whether the handler source in `stencil-aot/handlers.rs` is compiled with the codegen configuration that actually matters for copy-and-patch — no accidental frame-pointer overhead, correct target-cpu feature selection, no missed inlining across the tail-branch boundary that `build.rs` extracts. Use `llvm-mca`/`perf` against the extracted bytes to verify the assumptions the extractor and [[02-rustc-aot-templates]] depend on actually hold in the compiled output, not just in the source-level handler design.

This is infrastructure hygiene (a regression watch) rather than a new optimization mechanism: a codegen regression here (a toolchain upgrade changing inlining heuristics, a missing `target-cpu` flag) silently taxes every single stencil in the system, including every other task in this file that assumes AOT-extracted bytes are near-optimal for their shape. [[52-maximize-aot-codegen-power]] is the companion offensive task — it pushes codegen configuration forward; this task's baseline is what catches it drifting backward.

Acceptance: a documented baseline of expected instruction counts/latency for the current template set, checked against `llvm-mca` output; a regression test or CI check that flags a codegen change altering extracted template byte count or instruction mix beyond a defined threshold; findings from the audit either confirm current flags are correct or produce a concrete `build.rs`/handler-source fix.

Current work: collect native stack samples from the exact accepted task-97 executable
on long Crypto and Navier-Stokes runs. This distinguishes time in copied executable
stencils, the shared semantic block kernel, value ownership, computed-property helpers,
and benchmark harness code before choosing the next codegen boundary. Sampling is
diagnostic-only and must not be conflated with timed acceptance evidence.

Initial evidence is stored in `reports/native-samples/crypto.txt` and
`reports/native-samples/navier-stokes.txt`. In Crypto, `dyn_block_step_impl` accounts
for 1,156 of 2,185 top-of-stack samples and `Value` drop glue for 533 (24.4%); direct
computed get/set helpers account for only 87 and 31. In Navier-Stokes, the block kernel
accounts for 1,575 samples, drop glue for 370, computed get for 120, and computed set
for 37. The actionable boundary is therefore ownership-aware slot overwrite inside
the block kernel, not merely inlining computed-property helper calls. [[100-immediate-slot-overwrite]]
tracks that implementation.
