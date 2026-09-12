# 02 — rustc/LLVM AOT stencil templates

Status: complete

Rust macros define the AOT handler family once. The build extracts rustc/LLVM-produced handler bytes and relocations, verifies the required tail transfer, and packages those bytes as templates. Existing generated numeric templates include load and add/subtract/multiply/divide operations.

Evidence: `stencil-aot/handlers.rs`, the stencil extractor in `build.rs`, and composed-execution tests.

Rule: runtime compilation selects, copies, and patches prebuilt templates; it does not perform instruction selection, register allocation, or benchmark-specific assembly generation.
