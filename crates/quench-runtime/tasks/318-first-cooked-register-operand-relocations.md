# 318 — First cooked register-operand relocation slice

Status: complete

Implement the first bounded slice of [[316-typed-operand-holes-and-site-load-erasure]]:
rustc/LLVM cooks numeric-region arithmetic and comparison templates with named AArch64
unsigned-immediate operand holes. Final linking patches destination, left, and right
register-file byte offsets directly into load/store instructions, so those leaves do not
load ordinary operands from `InlineSite` at runtime.

The shared schema defines named hole kinds, placeholder slots, value width, instruction
masks, shifts, and immediate limits. The cooker records typed relocation offsets; the
linker validates alignment, range, and instruction form before replacing the immediate.
The burned `add` leaf is 44 bytes instead of the former 52-byte descriptor-reading leaf.

This bounded slice now covers numeric binary/comparison register operands plus encodable
local load/store, register move, unary plus, negate, and bit-not operands. Literal, IC
pointer, direct-target, wide-offset, and explicit object-file symbol relocations remain
under Task 316. The eventual form should be role-qualified
`CookedFixup { offset, operand_id, form }` records generated from one opcode schema, with
an expected-occurrence manifest and a wide/data-atom fallback.

Measurement: a three-repetition complete-suite A/B against the Task 307 checkpoint in
`reports/task317-burned-register-operands-ab-3` measured 2098.24 -> 2086.33 (-0.57%).
Component changes were small except a noisy Splay regression. This is neutral evidence:
three burned operand roles inside a subset of numeric leaves do not remove the dominant
generic-block and call-helper seams. Do not accept the feature as a score optimization
until the ordinary operand plane is covered and remeasured.

The expanded slice was then measured against the immediately preceding burned-register
binary in `reports/task318-expanded-operand-burning-ab-3`. Three complete-suite
repetitions measured 2045.48 -> 2042.17 (-0.16%). Component deltas were Richards -0.88%,
DeltaBlue +0.54%, Crypto -0.31%, RayTrace +0.68%, Earley-Boyer -0.12%, RegExp -1.95%,
Splay +1.29%, and Navier-Stokes -0.51%. This decisively classifies additional operand
patching by itself as neutral. Keep the validated relocation substrate; do not spend the
next experiment adding more isolated leaves.

Validation: the generated manifest names every supported operand role and the cooker
fails closed on missing, duplicate, extra, or wrong-kind relocations. Release and
GC-stress suites both pass 115/115 tests. The executable test
`cooker_burns_local_copy_move_and_unary_operands` exercises the newly covered path.

Acceptance met for this bounded slice: every supported template has an expected-hole
manifest; missing, duplicate, extra, wrong-kind, wrong-form, misaligned, and out-of-range
patches fail closed; release and GC-stress tests pass; covered operand loads/stores are
the patched instructions themselves; complete-suite A/B stays within component floors.
The score result is neutral and does not change the accepted 2083.86 checkpoint.
