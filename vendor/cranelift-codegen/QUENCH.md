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
Σ0(a) rotates on x64's two rotate ports). `#![allow(warnings)]` keeps the workspace's `-D warnings`
from failing on upstream lints that a registry dependency's capped lints
would hide. `QUENCH.patch` records these changes against the published
0.134.3 crate source.
