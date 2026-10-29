# Quench integration for Cranelift codegen

Upstream source: Cranelift `cranelift-codegen` 0.134.3, crates.io archive
SHA-256 `709f4b7c0fb57d952658d5b5c07fbdc4149acd7b7f0de9678ae754b9c981949a`,
upstream revision `5554cc1a651da536af2cc46c7324bdc085b162e3`, path
`cranelift/codegen`. The matching upstream Apache-2.0 WITH LLVM-exception
license is retained.

The local patch aligns the header of each small loop without calls, and of
each loop whose header dispatches through a jump table (an interpreter's
switch), to the target's `small_loop_alignment` (x64: one 64-byte cache
line; other targets unchanged), so a hot loop's code sits in the fewest
fetch windows instead of wherever the preceding code leaves it. Upstream Cranelift aligns
only functions.

The patch also scopes the elaborator's loop stack to the dominator tree:
upstream pops a loop when it enters a block outside it, so a loop exit
visited before the loop's body (both children of the header) hid the loop
from the body, and nothing was hoisted out of it (mandel's inner loop
recomputed a loop-invariant three-operator chain each iteration). Each
dominator-subtree exit now restores the loop stack its block was entered
with.

The elaborator also orders the operands of a binary instruction by
height (the longest chain of pure operations producing each): when the
second operand's chain is longer by `CRITICAL_LEAD` operations or more, its
instructions are placed first. Upstream places operands in argument order,
so a recurrence's critical chain could follow off-path work that competes
for the same execution ports (SHA-256's round: Σ1(e) rotates queued behind
Σ0(a) rotates on x64's two rotate ports).

For an operator with a float result, a rematerialized constant argument
(or a clone of one made for another dominator subtree) no longer bounds the
loop level the operator is hoisted to: the constant is copied into the
operator's block anyway, and counting where its first copy landed pinned a
loop-invariant float chain inside a loop (mandel recomputed each row's
imaginary coordinate per pixel). Integer operators keep the upstream rule,
since hoisting their chains measured slower through register pressure.
A scalar `bitcast` of an integer constant folds to the float constant with
those bits, which rematerializes at each use instead of staying live (a
register's float view of an integer zero had been spilled and reloaded per
pixel in mandel).
On x64, a 32-bit load (`load.i32`, `uload8`/`uload16` to i32) counts as
zeroing its register's upper 32 bits, as `movl`/`movzx` do, so widening its
result for an address needs no `movl` copy (coremark's list walks carried
one on each pointer-chasing step).
On x64, a 32- or 64-bit `smul_overflow` lowers to the two-operand `imul`,
which sets OF exactly when the signed product does not fit; upstream used the
one-operand form, which pins its operand and result to `rax` and clobbers `rdx`
with the unused high half (V8-v7 Crypto's am3 loop spilled around it).
On x64, a `brif` on the overflow flag of a 32- or 64-bit `sadd_overflow`,
`ssub_overflow` or `smul_overflow` placed right before it jumps on the OF that
arithmetic sets (`jo`); the arithmetic then materializes the flag only where
another lowered instruction reads it. Upstream sets a byte from OF and tests it
(`seto`/`test`/`jne`), two extra instructions on each guarded int32 operation.
`#![allow(warnings)]` keeps the workspace's `-D warnings`
from failing on upstream lints that a registry dependency's capped lints
would hide. `QUENCH.patch` records these changes against the published
0.134.3 crate source.
