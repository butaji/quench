# 386 — Adjacent tail-connector physical relaxation

Status: complete

Remove a physical control-flow seam without changing the immutable stencil expression or
its categorical connector. When a final AArch64 tail-branch relocation targets the next
instruction word, materialize that identity edge as the architecture's named `NOP` word.
Non-adjacent, backward, conditional, symbolic, and slow edges retain ordinary branches.
The transformation is general over final stencil layout; bytecode identity, benchmark
identity, runtime values, counters, and hotness do not participate.

The accepted Task 381 catalog preflight found that 98 of 143 inspected cooked templates
place their next-continuation branch in the final instruction word. The runtime previously
patched every one to `b +1`, even when the successor was physically adjacent. The retained
candidate changes only `patch_tail_branch`; its unit test proves adjacent edges become
`a64_abi::NOP` while a two-word edge remains an encoded `b +2`. The candidate artifact is
`/tmp/deegen-task386-fallthrough-candidate`, SHA-256
`ed25e35e0f34bbcf89291af93fe5c24045021e33fddf757c6fffed78e979c0a3`.

A more aggressive second realization removed the four-byte connector word and translated
all later holes and labels to a fixed point. Semantic tests passed, but the three-pair
100 ms screen in `reports/task386-erased-fallthrough-ab-3/comparison.txt` measured -1.33%
aggregate and -11.34% Navier-Stokes. This demonstrates that compacting independently
cooked bodies changes instruction alignment/layout enough to dominate the saved word. It
was rejected and removed. Its preserved artifact is
`/tmp/deegen-task386-erased-fallthrough-candidate`, SHA-256
`a1da3128ff118b3f65d04eaf46d6614f4a8335a87aa484bc49d02119a01f1e2e`.

The same-width candidate's first three-pair 100 ms screen was +0.10%. A stronger five-pair
200 ms screen in `reports/task386-adjacent-nop-ab-5/comparison.txt` measured +1.03%
aggregate; Crypto was -2.28% and every suite remained above the development floor. The
machine was visibly slower across both arms during this screen, so this is preflight rather
than acceptance evidence.

The randomized nine-pair upstream-exact gate in
`reports/task386-adjacent-nop-exact-ab-9/comparison.md` rejected the same-width realization:
2253.66 -> 2252.05, **-0.07%**, with a 95% paired-bootstrap interval of
**[-0.82%, +0.60%]**. DeltaBlue and Crypto also had individually negative intervals.
All 144 release tests passed before measurement. The source was reverted after the failed
gate; this task is complete as a measured rejection. An adjacent unconditional connector
is not expensive enough to prioritize ahead of eliminating value materialization across a
coarse region.
