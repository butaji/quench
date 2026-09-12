# 83 — Runtime semantic basic-block frequency profile

Status: complete

Add disabled-by-default `DEEGEN_BLOCK_STATS`. At each generic block-kernel entry it
counts the stable `(code identity, bytecode pc)` site and computes that site's semantic
shape only once. JIT statistics aggregate those site counts by shape in
`block_kernel_entries`. Literal categories and unary/binary operation kinds are part of
the shape, while source names, property names, register numbers, and branch offsets are
deliberately excluded so the evidence identifies general bytecode stencil families
rather than benchmark-source fragments.

The initial opcode-only full-suite record is `reports/current-block-profile.jsonl` and
the refined semantic record is `reports/semantic-block-profile.jsonl`. They prove the
current performance ceiling is structural: direct inline entries are zero in the
accepted configuration, while the generic Rust block helper repeatedly executes these
dominant reusable shapes:

- local/null strict equality and conditional branch: about 970 thousand block entries;
- local/number loose equality and conditional branch: about 554 thousand entries;
- local/name comparison and conditional branch: about 1.8 million entries;
- local/literal update followed by comparison: about 1.7 million entries;
- large numeric/array loop bodies in Crypto and Navier–Stokes: about 1.6 million and
  1.3 million entries respectively.

The profiler is observational infrastructure only: no counters or shape construction
run unless the environment option is enabled. Refined semantic-kind output replaces
the initial coarse opcode-only labels and is the selection evidence for task 36's next
coarse, typed block stencils.

Acceptance: the full supported suite remains correct with profiling enabled; emitted
statistics are valid JSON; normal tests pass with profiling disabled; index and task
metadata agree.
