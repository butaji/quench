# 213 — Canonical effective-address and memory-operation stencils

Status: planned

Introduce one quoted, architecture-neutral memory operand:

`MemoryRef { base, index, scale, displacement, representation, proof }`.

Locals, fixed property slots, dense elements, and binding cells lower to this same data
shape. `scale` and `displacement` use named representation/layout constants; no machine
offset or tag mask is repeated as a literal in selectors or handlers. Alias, bounds,
shape, lifetime, and ownership requirements remain explicit in `proof` rather than
hidden in the address calculation.

Task 149's Rust macros derive finite rustc/LLVM-cooked variants that consume a
`MemoryRef` directly: load-and-unbox, memory-operand arithmetic/compare where the target
supports it, and box-and-store. This removes temporary `Value` publication and redundant
base/index arithmetic between otherwise composable stencils. Task 55's architecture
module answers which address forms are legal; runtime linking still only selects,
copies, and patches prebuilt bytes.

In categorical terms, `MemoryRef` is a single quoted object interpreted by two lowering
functors: the generic semantic kernel and the target stencil catalog. Composition is
still over typed connector contexts; the target-specific instruction form is derived at
the final AOT cooking edge and never leaks into bytecode semantics.

Acceptance: local/property/dense accesses all derive their addresses from the canonical
form; AArch64 disassembly shows at least one indexed dense load and one fixed-offset
property load folded into the consuming operation with fewer instructions and no extra
frame round-trip; alias/shape/bounds/GC-keepalive tests cover every proof field; a
non-representable address falls back to ordinary composable load/operate stencils; full
V8v7 A/B improves. Register, displacement, scale, and variant budgets are named
constants.

Primary sources:
- SpiderMonkey's optimizing pipeline explicitly includes Effective Address Analysis and
  load-with-unbox folding: <https://firefox-source-docs.mozilla.org/js/MIR-optimizations/index.html>
- JavaScriptCore's B3-to-Air selector fuses loads, stores, immediates, addresses,
  compares, branches, and arithmetic while keeping the low-level IR compact:
  <https://webkit.org/blog/5852/introducing-the-b3-jit-compiler/>
- LLVM's AArch64 backend exposes a dedicated load/store optimization stage:
  <https://llvm.org/docs/doxygen/AArch64TargetMachine_8cpp_source.html>
