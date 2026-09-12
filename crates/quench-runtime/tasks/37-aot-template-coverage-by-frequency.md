# 37 — Expand AOT template coverage ranked by measured frequency

Status: in_progress

Complements [[36-direct-opcode-stencils]]: use `scripts/perf-cycle.sh rank` against the full V8v7 suite to rank opcodes by observed `KERNEL_EXIT` frequency, and systematically promote the highest-frequency generic-helper opcodes to rustc/LLVM-extracted templates in `stencil-aot/handlers.rs`, following the extraction pattern already established in [[02-rustc-aot-templates]]. This is process discipline as much as implementation: every promotion must be justified by the rank output, not by guessing which opcode "seems hot."

Distinct from [[36-direct-opcode-stencils]]'s specific closed subset (load/store/move/guarded-arithmetic/branch/return): this task is the ongoing, repeatable procedure for whatever the *next* highest-frequency generic-helper opcode turns out to be once 36's subset is done, so template coverage keeps advancing by evidence rather than stalling once the initial list is exhausted.

Acceptance: a template promotion is preceded by a `perf-cycle.sh rank` artifact showing its frequency rank; each promotion is accepted only via the existing A/B `accept` harness ([[05-performance-harness]]); the coverage report ([[04-bytecode-coverage-map]]) shows the promoted opcode moving from `KERNEL_EXIT` to `INLINE_STENCIL` across the suite, not just in a synthetic test.

Current instrumentation: `DEEGEN_BLOCK_SHAPE_TRACE=1` emits the canonical lowered opcode sequence for every compiled basic block and marks loop-region blocks. This is compile-time observation only; it neither counts runtime heat nor changes template selection. Use it together with runtime opcode ranking to choose general fused templates that amortize frame/site loads, rather than copying the generic `dyn_run` matcher or guessing from benchmark source text.

This task supplies the front-side (AST/frequency demand) half of [[53-bidirectional-stencil-cooking]]'s methodology; a promotion should be written against that task's matching back-side (LLVM lowering) capability entry, not authored independently of it.

Promotions now accepted through this routine include the terminal and unconditional
transfer vocabulary in [[109-normalized-basic-block-stencil-vocabulary]] and constant
condition erasure in [[110-constant-condition-control-stencils]]. [[113-static-store-run-kernel]]
proved that wrapping a frequent form in another decoding helper is insufficient. The next
experiment, [[114-dead-local-local-condition-stencils]], therefore cooked a frequent
four-op block into direct rustc/LLVM machine code and erased its dead temporaries. It was
also rejected: repeated local-boundary proofs expanded the block to 104 bytes and made
the dominant Navier-Stokes use -1.29% slower. [[115-validated-local-context-stencils]]
tracks moving that invariant to the function boundary before attempting more templates.
[[117-composable-subblock-stencil-regions]] then proved that even a correctly segmented,
direct four-op update island is too fine-grained: its 100-byte cooked body and connector
transfers reduced Richards and Crypto beyond the smoke floor. Future promotions must
cover coarse expression/loop regions and retain values across operations, not increase
the count of independently guarded little stencils.

[[118-borrowed-numeric-stencil-arguments]] adds a second selection rule: compiled-image
counts are not runtime coverage. A candidate is eligible for optimization only after
separate counters prove that dispatch considers and enters it. Task 118's borrowed
argument interface was correct, but V8v7 produced zero numeric call candidates, so it
was reverted without a long performance comparison.
