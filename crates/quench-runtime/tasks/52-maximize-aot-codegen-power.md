# 52 — Maximize rustc/LLVM codegen power in the AOT stencil-cooking step

Status: in_progress

Distinct from [[43-aot-codegen-quality-audit]] (verifying current codegen isn't regressing) and [[30-stencil-register-allocation-quality]] (register allocation within one stencil's own logic): this task actively pushes the rustc/LLVM invocation in `build.rs`/`stencil-aot/handlers.rs` (currently `-Copt-level=3 -Cpanic=abort -Ccodegen-units=1 -Crelocation-model=pic -Zfunction-sections=yes`, per-crate `lto = "thin"`) toward the most aggressive codegen the copy-and-patch extraction contract can tolerate, rather than the conservative flag set that happens to work today.

Every step here must preserve the two invariants `build.rs`'s `extract_catalog` depends on: each handler remains a single, separately-extractable symbol (LLVM must not inline handler bodies into each other or split one handler across multiple functions), and each handler's tail transfer stays a recognizable, patchable branch (`AARCH64_TAIL_BRANCH_OPCODE`). "Maximize" means push everything *inside* that boundary, never blur the boundary itself.

Concrete steps, ranked by expected payoff:

1. **Fat LTO for the handler object, not just thin.** The main binary's `Cargo.toml` uses `lto = "thin"`; the standalone `rustc` invocation for `stencil-aot/handlers.rs` in `build.rs` does no LTO configuration at all beyond default single-crate codegen. Since handlers are compiled as one `--crate-type=lib` unit already (`-Ccodegen-units=1`), evaluate `-Clto=fat` for this specific invocation — full-object optimization within the handler crate, bounded by the per-handler extraction invariant above (verify no handler gets merged/deduplicated by LLVM's function-merging into a shape `extract_catalog` can't parse).

2. **Pin `target-cpu`/`target-feature` deliberately, not implicitly.** No `-C target-cpu=` flag is currently passed, so codegen uses the generic AArch64 baseline. Pick and pin an explicit target-cpu matching the actual deployment target (not `native`, which would make extracted templates non-portable across build machines and break reproducibility) — this unlocks real instruction selection improvements (wider load/store pairing, better addressing modes) that the generic baseline conservatively avoids. Record the pinned value next to `PINNED_RUSTC_RELEASE` so it's version-controlled the same way the toolchain is.

3. **Release-profile matrix without benchmark-trained code.** Isolate and A/B the main
   crate's `panic = "abort"`, ThinLTO versus fat LTO, and any cross-crate visibility
   changes. Cargo documents that LTO enables whole-program LLVM analysis and that fewer
   codegen units can improve generated code. V8v7 is only the acceptance measurement;
   never feed V8v7 execution profiles into rustc/LLVM, because that would shape the
   supposedly general template catalog for the target benchmark. A future production
   workload PGO mode, if desired, is a separate deployment feature and cannot count
   toward the 10000 gate.

4. **Vectorization-friendly handler source for the numeric templates feeding [[39-vectorized-numeric-stencils]].** Verify LLVM's autovectorizer actually fires on the arithmetic handler bodies (`am1`-style multiply-accumulate shapes, elementwise loops) by inspecting the extracted object with `llvm-mca`/objdump for SIMD instruction presence, not just assuming `-Copt-level=3` is sufficient — loop bodies split across the handler/tail-branch boundary can defeat autovectorization even at `-O3` if the vectorizer can't see past the artificial per-op boundary. Where the boundary blocks vectorization, [[39-vectorized-numeric-stencils]]'s explicit SIMD-lowered handler variants are the fallback; this step is about not leaving free vectorization on the table for the general case first.

5. **Explicit `-C llvm-args` tuning for inlining threshold and merge-functions behavior.** Since correctness here depends on handlers *not* merging/inlining into each other, explicitly pass `-C llvm-args=-inline-threshold=<value>` tuned for "inline freely within a handler's own call tree, never across handler symbol boundaries" and confirm `-Z merge-functions` (function merging, which could collapse two byte-identical handlers into one symbol and break `extract_catalog`'s one-symbol-per-template assumption) is off or accounted for.

Acceptance: each change is validated via [[43-aot-codegen-quality-audit]]'s `llvm-mca`/`perf` baseline (no invariant violation: symbol count, tail-branch recognizability, and per-handler extractability all unchanged) before being accepted through the standard A/B harness ([[05-performance-harness]]); a documented aggregate score improvement attributable to the codegen change alone (isolated from any semantic/guard-logic change in the same measurement window); the pinned `target-cpu` and any `-C llvm-args` are recorded in `build.rs` with the same version-pinning discipline as `PINNED_RUSTC_RELEASE`, so the "maximize" configuration is reproducible, not incidental.

Round-fifteen correction: "maximize" does not mean `-O3` by definition. CPython's
current copy-and-patch cooker uses `-Os` because some higher-level standalone-function
transforms are counterproductive after snippets are concatenated. Task 273 owns the
isolated cooker-pipeline matrix; this task consumes its measured result rather than
choosing by optimization-level name.

Current experiment: evaluate an explicit `apple-m4` target for both the main runtime and
the separately cooked stencil-handler crate. The benchmark host identifies itself as
Apple M4 and nightly rustc lists `apple-m4` as a supported target CPU. The experiment
must preserve the exact task-97 executable as its baseline, pass the stencil extractor's
symbol/relocation assertions and the semantic suite, and clear the balanced A/B gate
before the target is committed to repository configuration.

Runtime-wide result: rejected. `RUSTFLAGS=-Ctarget-cpu=apple-m4` passed all 41 tests
and reduced the executable from 2,990,672 to 2,940,464 bytes, but the alternating
three-repetition full-suite comparison in
`reports/apple-m4-main-ab/comparison.txt` regressed the aggregate from 693.874 to
692.726 (-0.17%). Six suites were flat or slower; only Crypto (+1.61%) and Splay
(+2.20%) improved. A smaller executable is not sufficient evidence, so no global
runtime target flag is retained.

Next isolated experiment: apply the named CPU target only to `build.rs`'s standalone
stencil-handler compilation. This tests the immutable templates independently of the
main semantic-kernel layout.

Stencil-only result: also rejected and reverted. Adding
`-Ctarget-cpu=apple-m4` solely to the standalone AOT handler invocation passed the
extractor and all 41 tests, but produced a final executable with exactly the same
2,990,672-byte size and SHA-256 as the generic-target accepted binary. Since the
selected template bytes did not change, another performance run could not distinguish
the executables. The repository retains no target pin. Future work on this task must
inspect a stencil whose generated instructions actually differ before paying for a
full-suite A/B.
