# 422 — Runtime-constant range shifting for cooked relocations

Status: planned

Deegen burns bytecode operands into stencil instruction streams as external-symbol
addresses rather than decoding them at runtime, then runs a dedicated range-analysis pass
over every derived runtime constant (e.g. `lhs_slot * 8`) to guarantee it lands inside the
x86-64 ABI's valid external-symbol range `[1, 2^31 - 2^24)` after relocation, rewriting
expressions (such as a `+ 1` shift) to move a constant back into range when a fold would
otherwise place it outside.

Task 102 (reject unrepresented AOT relocations) only rejects operands that don't fit; it does
not shift a derived constant's representation so a fold that would otherwise be rejected can
still be admitted safely. Add the range-analysis and range-shifting pass to the stencil
cooker: given a derived runtime-constant expression and rustc/LLVM's relocation model, either
prove it stays in the safe range or transform it (documented, invertible) into one that does,
before falling back to Task 102's rejection.

Acceptance: at least one previously-rejected-by-102 constant expression now admitted via a
documented shift, with a decode-side test proving the shift is correctly inverted; a
mutation/counterexample test that an out-of-range constant is still rejected when no valid
shift exists; full correctness suite.

Primary source: sillycross, "Building a baseline JIT for Lua automatically"
<https://sillycross.github.io/2023/05/12/2023-05-12/> (Runtime Constants via External Symbols
section).
