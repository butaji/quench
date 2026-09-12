# 123 — Traced numeric/dense loop stencil

Status: complete

Current work: consume each Task 120 quote once during linking, compose the Task 122
leaves into a single labeled region image, put the Task 121 guard on the external entry,
and route the internal backedge directly to the already-validated loop body.

Compose [[120-typed-numeric-dense-region-ir]], [[121-hoisted-numeric-dense-guard]], and
[[122-rustc-aot-numeric-region-templates]] into one linked memory function per eligible
loop. Symbolic labels close the back-edge as a trace; internal next holes resolve during
composition; frame/literal/guard holes resolve only at final link. The final function
contains a single guard entry, a straight-line composed body, direct local branches,
and one canonical slow exit.

Composition remains closed at operation, block, loop, function, and program levels.
The linker walks the quoted value once, copies each selected template once, patches all
holes, validates label demands/promises, and seals executable memory. There is no
hot-path detector: every eligible loop receives this stencil on first compilation.

Acceptance: full semantic coverage and release tests pass; counters separately prove
eligible regions, linked regions, guard successes/failures, and executed iterations;
native sampling shows the targeted loops no longer dominated by `dyn_block_step` or the
generic opcode switch; accept only after an exact alternating full-suite A/B improves
aggregate with no suite below the standing -5% floor.

## Result: accepted

The linker now consumes every eligible quote on first compilation, emits one external
guard label plus private body labels, composes the macro-generated AOT leaves as a flat
loop morphism, closes internal branches/backedges symbolically, and appends canonical
public fallback blocks. A failed guard executes the whole original region and cannot
enter an unchecked private label. The internal backedge returns directly to the body,
so validation occurs once per external region entry rather than once per iteration.

Ownership is explicit at the connector: first-write registers and locals are cleared
once, dense carrier loads become identity leaves, and the guard installs owned receiver
copies before unchecked execution. Runtime trace configuration is captured at link time;
normal guards no longer perform an environment lookup. `DEEGEN_NUMERIC_REGION_STATS=1`
selects a separate counted-backedge template, leaving normal images counter-free.

Evidence:

- 61 release tests and the complete smoke suite pass.
- `reports/task123-crypto-region-stats.txt`: 15 linked regions, 137,219 guard
  successes, 155 failures, and 1,476,625 native stencil iterations.
- `reports/task123-navier-region-stats.txt`: 16 linked regions, 12,760 successes,
  3 failures, and 1,754,696 native stencil iterations.
- `reports/task123-traced-region-ab-6/comparison.txt`: exact six-repetition alternating
  aggregate 827.695 → 1006.03 (+21.55%). Crypto improves 20.67% and Navier–Stokes
  346.44%; every other suite remains above the standing -5% floor.

Accepted executable: `/tmp/deegen-task123-traced-numeric-dense-loop`, SHA-256
`a59924a73ac0865c48c5d8777d63944d5f3f772911c907611e69c963182113ab`.
