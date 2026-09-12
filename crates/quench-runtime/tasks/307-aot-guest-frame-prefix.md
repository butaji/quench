# 307 — AOT-visible guest-frame prefix as an offset-zero record

Status: complete

Extract the machine-code-visible prefix of `DynFrame` into one explicit
`#[repr(C)] GuestFrameHeader` at offset zero. The header contains exactly the fields
addressed by rustc/LLVM-cooked stencils; Rust-owned frame state remains after it. Every
field position is derived from named word-offset constants, and the total header size is
checked against `GUEST_FRAME_HEADER_WORDS` at compile time.

This is a representation boundary, not yet the pointer-bump guest stack from Task 146.
In particular, `result: Value` still has ownership semantics, so this first record is
AOT-layout-compatible but is not claimed to be a `Copy` POD activation. The next slice
must move ownership behind an explicit raw-word boundary and separate the remaining
Rust sidecar before cooked code may allocate activations itself.

`DynFrame` dereferences to the prefix to keep existing field access source-compatible.
The operation is zero-allocation and adds no runtime pointer indirection: the nested
record starts at byte zero.

Validation on 2026-09-10:

- `cargo fmt -- --check` and release checking pass;
- all 108 release tests pass normally and with `DEEGEN_OBJECT_GC_STRESS=1`;
- executable segment sizes are identical to the frozen Task 305 baseline;
- the three-pair complete-suite comparison in
  `reports/task307-guest-frame-prefix-ab-3/comparison.txt` measures
  2067.80 -> 2083.86 (+0.78%), with all eight components above -0.20%.

The result is accepted as neutral structural infrastructure. It does not claim a score
gain and does not make the existing direct-call feature acceptable.
