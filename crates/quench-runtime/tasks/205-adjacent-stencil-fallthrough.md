# 205 — Adjacent stencil fallthrough linking

Status: complete

Eliminate the physical tail branch when a stencil's `Next` connector resolves to the
instruction immediately following that branch. Sequential composition still records
and resolves the same categorical edge, but the final AArch64 link writes the canonical
`NOP` instruction so execution falls directly into the neighboring stencil. Forward,
backward, conditional, slow, and non-adjacent edges retain their patched branches.

This is general bytecode-driven composition: it depends only on final code offsets, not
on opcode names, source patterns, benchmark identity, or execution frequency. The
architecture opcode is named in `a64_abi`; no linker magic number is introduced.

Acceptance: a linker unit test distinguishes adjacent fallthrough from a non-adjacent
branch; all semantic tests pass; generated composed regions execute correctly; a full
alternating V8v7 comparison determines whether the change remains enabled.

Primary source: Copy-and-Patch adjacent-stencil fallthrough and continuation linking,
<https://arxiv.org/abs/2011.13127>.

## Result

The implementation and its linker law test passed all 86 semantic tests, but the
five-repetition complete-suite comparison in
`reports/task205-adjacent-fallthrough-full-ab-5/comparison.txt` regressed the aggregate
from 1799.08 to 1782.89 (-0.90%). Splay regressed 3.60%, RayTrace and RegExp each
regressed 1.74%, and the numeric suites were effectively flat. The rewrite is therefore
rejected and the linker again emits an explicit branch even for an adjacent target.

This does not invalidate larger fallthrough-aware layout work: erasing one already
predictable branch while retaining each stencil's separate instruction footprint is
insufficient. Revisit only as part of Task 157's costed multi-granularity freezing,
where adjacent bodies can also share loads, guards, and connector state.
