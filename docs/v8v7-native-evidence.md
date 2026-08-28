# V8_v7 native evidence ledger

This ledger is one source of truth for performance decisions.  A row is not
permission to optimize: `Observed` means measured, `Unknown` means the next
measurement is named, and only `Proven` semantic facts may enter a fast path.

## Reproducible baseline

| fact | value |
| --- | --- |
| executable | `target/release/quench-node`, measured 2026-08-28 |
| suite command | `node quench-bench/run-quench-runtime.mjs --quench target/release/quench-node --runs 1 --timeout-ms 240000` |
| Richards | valid, 66.6, 3131.6 ms |
| DeltaBlue | valid, 40.3, 6719.5 ms |
| Crypto | valid, 70.7, 40049.2 ms |
| Raytrace | valid, 57.6, 44501.0 ms |
| Earley-Boyer | valid, 52.9, 239123.5 ms |
| RegExp | timeout, 240038.4 ms |
| Splay | valid, 37.4, 36022.4 ms |
| Navier-Stokes | valid, 166.0, 31516.5 ms |

That historical aggregate score was unproven.  Do not derive a score from a
partial suite.

## Current valid native baseline

The current dirty worktree was built in an isolated target directory with the
production profile plus `target-cpu=native`, DWARF level 2, and no stripping.
The executable and dSYM share UUID `258F94E4-A527-30FB-9251-C948A6E9DD3E`.
DeltaBlue first passed directly at 56.7.  Its subsequent complete-suite run
was:

| fixture | validity | score | wall time |
| --- | --- | ---: | ---: |
| Richards | valid | 96.5 | 3.142 s |
| DeltaBlue | valid | 55.1 | 5.451 s |
| Crypto | valid | 83.0 | 38.079 s |
| Raytrace | valid | 114.0 | 22.021 s |
| Earley-Boyer | valid | 105.0 | 90.755 s |
| RegExp | **timeout** | — | 240.003 s |
| Splay | valid | 116.0 | 9.454 s |
| Navier-Stokes | valid | 242.0 | 20.911 s |

The end-to-end command took 450.37 s.  macOS `/usr/bin/time -l` reported
1,684,062,208 bytes as maximum resident set size and 16,845,080 bytes as peak
memory footprint.  The former covers the benchmark runner process tree and
must not be misreported as Quench-alone RSS.  RegExp is the only remaining
aggregate-score blocker; Earley-Boyer is no longer one.

## Compiler and profiler artifacts

| fact | value |
| --- | --- |
| source snapshot | `3f78fc18c` (clean, but DeltaBlue is semantically broken) |
| native/DWARF binary UUID | `ABA7475F-563E-3308-B3DA-DB81E493CB50` |
| profile mode | `-C target-cpu=native`, `-O3`, Thin LTO, one CGU, panic abort, dSYM |
| Time Profiler | 18.16 s RegExp block; 11,138 samples, 6,837 in Quench |
| hottest resolved frames | `functions::validate_leaf_depth`, `functions::execute_shape_kernel` |
| rejected configuration | Thin LTO made Crypto exceed 240 s; fat LTO release completes in 40.0 s |
| compact execution value | `TaggedValue(u64)`, compact `#[repr(u8)]` tag |
| general semantic value | broad heap-owning `Value` enum; crossing into it must be measured |

The profile source snapshot is useful only for physical facts.  Its DeltaBlue
Projection 2 error proves it must not be used as a semantic baseline.

## 50-item evidence matrix

Status: **Observed** is current evidence, **Unknown** names required data,
and **Rejected** is an A/B result.  `P` means Time Profiler plus dSYM;
`C` means CPU Counters; `A` means allocation trace; `D` means disassembly.

| # | status | current fact / next decisive measurement |
| ---: | --- | --- |
| 1 | Observed | `TaggedValue(u64)` is the execution transport; P must count `Value` crossings in RegExp and Earley. |
| 2 | Unknown | C branch samples at tag decoding; do not reorder tags before this count. |
| 3 | Observed | flattened call loops erase loop facts; preserve only reusable semantic loop facts. |
| 4 | Observed | tag discriminator is `#[repr(u8)]`; D must show compare/jump, not helpers. |
| 5 | Observed | compact tag exists; measure register-file/object sizes before layout edits. |
| 6 | Observed | `Value` is a broad enum; P/A must prove copies in hot execution before redesign. |
| 7 | Observed | hot register word is eight bytes; D must check register instead of stack movement. |
| 8 | Unknown | P/D local register file vs pointer-chased environment loads. |
| 9 | Unknown | LLVM missed loop/vector remarks and P alias-heavy lines. |
| 10 | Unknown | compare a proven unique-borrow kernel only after item 9 identifies reloads. |
| 11 | Unknown | P `RefCell::borrow*` sample share in the two blockers. |
| 12 | Unknown | A/P `Rc` clone/drop sample share in the two blockers. |
| 13 | Unknown | arena handles require A/P proof that ownership traffic dominates. |
| 14 | Unknown | C L1D samples must locate pointer-walk hot loops. |
| 15 | Unknown | no SoA change until item 14 identifies a repeated subset-of-fields scan. |
| 16 | Unknown | object hot/cold split needs object-access cache evidence and one owner representation. |
| 17 | Observed | interpreter/function-plan code has many non-inlined paths; D symbol spans pending. |
| 18 | Observed | P shows repeated leaf-plan validation and shape-kernel work. |
| 19 | Unknown | D call-site audit for current `#[inline]` helpers. |
| 20 | Unknown | audit `#[inline(always)]` only when D shows code-size/I-cache cost. |
| 21 | Unknown | D must prove cold error/coercion code is outside numeric loop bodies. |
| 22 | Unknown | use D after each guarded fact to verify constant propagation. |
| 23 | Unknown | identify compile-time opcode/mode conditions that remain in D. |
| 24 | Unknown | identify emitted unreachable branches in D; remove only with semantic proof. |
| 25 | Unknown | compare one iterator/slice hot kernel assembly before rewriting abstraction. |
| 26 | Unknown | P indirect-call share; no trait-object conversion in fast path. |
| 27 | Observed | fixed register-file leaf specializations exist; profile their admission cost. |
| 28 | Unknown | D/nm function-size distribution before more monomorphization. |
| 29 | Unknown | D panic/bounds branches in hottest body kernels. |
| 30 | Unknown | compare slice iteration only after item 29 finds a live bound check. |
| 31 | Guarded | `get_unchecked` is forbidden until 29 proves a surviving bound check and fact proves bounds. |
| 32 | Unknown | D count invariant field loads per loop iteration. |
| 33 | Unknown | D loop-backedge/unroll evidence for numeric kernels. |
| 34 | Unknown | LLVM vectorization remarks for numeric-only loops. |
| 35 | Unknown | missed-vectorization reason must identify a removable semantic barrier. |
| 36 | Unknown | C branch behavior of tag/layout/property checks. |
| 37 | Guarded | probability reorder requires item 36 and stable guard-hit distribution. |
| 38 | Unknown | D opcode dispatch lowering: jump table vs comparison tree. |
| 39 | Unknown | inspect opcode density before renumbering; semantics must remain data-driven. |
| 40 | Observed | profile shows generic function-plan/shape dispatch is nontrivial. |
| 41 | Observed | only generic reusable superinstructions are admissible; no fixture recognition. |
| 42 | Unknown | D repeated frame/register loads in blocker body. |
| 43 | Unknown | D/A numeric local update path and `Value` reconstruction count. |
| 44 | Unknown | D/P separation of coercion, errors, and TDZ from ordinary numeric flow. |
| 45 | Unknown | Allocations trace by source line; RegExp match-result allocation is a candidate, not proof. |
| 46 | Observed | release uses panic abort; D must verify no unwind path in hot binary. |
| 47 | Rejected | native CPU code is retained; Thin LTO experiment was slower, not evidence against native CPU. |
| 48 | Observed | fat LTO is the current winning baseline; Thin LTO is rejected for now. |
| 49 | Observed | current fat LTO/one CGU release is the benchmark baseline; keep only after quiet A/B. |
| 50 | Observed | UUID-matched dSYM and raw trace are retained; every proposed win requires P/C/D/A evidence. |

## First native-code pass

`execute_shape_kernel` begins at `0x1003c77ec` in the UUID-matched native
binary.  Its 496-byte stack frame, calls to `shape_kernel_fact`,
`intern_object_layout`, and `virtual_builtin_cache_hit`, and repeated guard
branches are directly visible in `otool -tvV`.  This turns items 17, 18, 19,
29, 32, 36, and 42 from design hypotheses into a concrete audit target: the
admission/guard path is substantial, but it is not yet permission to delete a
single guard.  The measured RegExp samples place this function among the
hottest resolved frames.

The CPU Counters trace collected on 2026-08-28 is unusable for per-PC
decisions: its table of contents reports only the aggregate bottleneck modes
and no exported counter call-stack table.  Items 2, 14--16, and 36--37 remain
Unknown until a counter capture that exports source/PC samples is available.
Time samples and disassembly remain the current authoritative data.

Two isolated `quench-runtime` remark builds also produced no loop-vectorize
remarks or `.opt.yaml` sidecar: first with LLVM pass arguments, then with the
documented `rustc -C remark=loop-vectorize`.  Treat both as an unavailable
measurement in this toolchain, not evidence that no loop vectorized.  Items
9 and 34--35 continue to require a compiler/toolchain that exports the
remarks or direct disassembly of a specifically numeric hot loop.

## Valid RegExp profile: allocation and ownership dominate

The 20-second Time Profiler capture from the current UUID-matched binary
contains 8,117 raw samples.  Its most frequent active leaf frames include
`regex_automata` SipHash writes (275 samples), `Value` drop glue (259),
`Value::clone` (121), `String::clone` (90), `TaggedValue::decode` (52), and
Quench property/locals paths (`builtin_method`, `resolved_replacement`, and
`resolved_object_replacement`).  Native allocator/deallocator leaves
(`_xzm_free`, `_xzm_xzone_malloc`, `_free`, `_malloc_zone_malloc`, and
`_platform_memmove`) account for more than 1,800 leaf samples combined.

This closes the priority question for items 1, 6--7, 12, and 45: repeated
ownership traffic is measured in the only score-blocking fixture.  The next
candidate must remove a reusable allocation/copy from ordinary RegExp or
property semantics, retain ordinary fallback behavior, and then be measured
against this same full-suite baseline.  It does *not* justify result discard,
unchecked access, tag reordering, or a benchmark-specific path.

An isolated array copy-on-write experiment removed the artificial owner clone
before `Rc::make_mut` for unique arrays, while retaining the old replacement
path for aliased arrays.  DeltaBlue remained valid (57.7), but official RegExp
still timed out at 240.003 s.  It was reverted: safe local allocation removal
without a measured V8 score win is not retained while the score gate remains.

## RegExp causal trace and test262 import

An isolated production build with `execution-trace` ran the RegExp block in
16.5 s (score 231).  It recorded 2,705,797 cache hits and 108 misses; the
per-pattern combined cache/engine timing is sub-second, so compiling or
hashing patterns cannot explain the timeout.  The same run recorded
986,605 match-result allocations, 1,290,094 other allocations, 125,107,781
owned-word reads, 12,709,857 value decodes, and 2,710,528 built-in calls.
`string_replace_call` was admitted 437,155 times yet still traversed ordinary
call/result machinery.

`../quench-test262:test262` was fetched as `quench-test262/test262` at
`7bfaa7f56` and built in an isolated worktree.  It passes direct DeltaBlue
(55.7), carries the more complete native RegExp backend/correctness history,
but its official RegExp run also reached the 240 s watchdog.  It is a source
of semantic fixes, not unproven performance evidence.  Do not merge its
divergent runtime wholesale into the dirty worktree; evaluate its RegExp
commits through isolated compatibility and score gates.

## Verified ordinary global replace reduction

The ordinary global `RegExp[@@replace]` path had an avoidable semantic
detour: `global` alone forced `replace_with_exec`, materializing a JavaScript
exec-result array for every match, even after the observable `flags` and
`exec` reads had proved the ordinary built-in protocol.  The direct template
matcher already implements replacement-token expansion.  It now resets
`lastIndex` before the global scan (the observable built-in transition) and
is selected only when the existing flags/`exec` guards hold; sticky and
overridden-exec cases retain the complete exec path.

Against Node, `/a/g` with a nonzero initial `lastIndex` produced `"xbx"` and
final `lastIndex` zero, while `/(?:)/g` produced `"xaxbx"` and zero in both
engines.  The four focused runtime RegExp tests pass.  The isolated native
runBlock0 score rose to **273** in 13.99 s (29,327,360-byte max RSS), versus
the traced baseline range of 226--257.  This is a reusable allocation
reduction for ordinary JavaScript semantics, not discarded-result admission.

## Next experiment, ordered by impact / effort

1. Produce a DeltaBlue-valid dSYM build by isolating the dirty semantic repair.
2. Capture P+C+A traces separately for RegExp and Earley-Boyer on that exact
   binary, including total allocation count and L1D-miss samples.
3. If repeated leaf-plan validation remains material, model one versioned
   `FunctionPlanFact` per function/code identity.  Cache only proven immutable
   structural facts; guard all mutation and use ordinary execution on miss.
4. Before any result-elision, finish the explicit split between expression
   value, statement completion, and unobservable normal completion.  A
   discarded effect must still write the statement-completion register.

## Current full-suite physical baseline

The current native-CPU release binary completed the official V8_v7 programs
with a geometric score of **102.0**: Richards 75.9, DeltaBlue 45.4, Crypto
74.7, RayTrace 88.5, EarleyBoyer 98.3, RegExp 159, Splay 131, and
NavierStokes 252.  This is a valid complete score, rather than the former
RegExp-timeout partial result.

`/usr/bin/time -l` moves the physical-memory priority ahead of further
RegExp work: EarleyBoyer used 90.32 CPU s and peaked at 1,678,082,048 bytes;
Splay peaked at 501,743,616 bytes.  DeltaBlue peaked at 111,099,904 bytes;
every other program was at or below 35,438,592 bytes.  Earley also retired
1.848 trillion instructions, versus 111.9 billion in DeltaBlue.  Therefore
the next reusable optimization must target VM ownership/allocation on list-
and object-heavy ordinary code, with Earley first and Splay second.

An execution-trace pass found 193,460 rejected DeltaBlue leaf candidates.
They are all `CALLN_DISCARD_RESULT_FLAG` operations, not large-arity calls.
The leaf executor must continue to reject them: admitting them would again
conflate an unobservable call result with a statement-completion value.  This
rules out an otherwise tempting fixed-argument-buffer experiment.

The host denied the Instruments Allocations attachment (`-60007`), and the
full HashMap-based execution trace distorted Earley beyond several times its
90-second uninstrumented runtime.  These tools are unavailable as causal
evidence for Earley; retain per-process RSS/instruction counts and use a
lower-perturbation profiler or allocation counters before making a VM change.

## Earley-Boyer allocation and constructor evidence

The local Node oracle separates unavoidable workload allocation from Quench
retention: EarleyBoyer completed in 4.68 CPU seconds and 132,677,632-byte RSS;
Quench needed 90.32 CPU seconds and 1,678,082,048-byte RSS.  Splay's roughly
500 MB peak, in contrast, matches Node's 513 MB, so it is not the first
reclamation target.

The trace-only `QUENCH_EXEC_TRACE_HEAP_ONLY` lifecycle mode keeps per-opcode
HashMap tracing disabled.  In EarleyBoyer it saw 27,139,825 object allocations
and 26,237,456 drops before process teardown, with 25,914,058/25,769,067
environment allocations/drops.  A one-invocation split found the score gap in
the Boyer half: Quench 5.21 CPU seconds / 92.3B instructions versus Node 0.05
CPU seconds / 618M instructions.  Its ordinary trace recorded 258,568
two-field record constructions and 203,173 calls to one 84-op recursive
function.

The existing two-field record constructor fast path rejected any prototype
with methods, even where neither assigned field nor its descriptor appeared
anywhere in the ordinary prototype chain.  It now traverses that chain,
rejecting replacements and all field/descriptor/deletion conflicts, while
retaining the complete interpreter fallback.  Node and Quench agree for an
ordinary custom prototype, own setter, and inherited setter.  Isolated Boyer
improved from 5.21 to **2.50 CPU seconds**, 92.3B to **52.8B instructions**,
and 98.5 MB to **80.8 MB RSS**.  The official EarleyBoyer score improved from
98.3 to **105**, with RSS down from 1.68 GB to **1.58 GB**.

An item-1 code-view experiment cached flattened loop source in a shared lazy
`FunctionCode` cell.  It did not reduce retired instructions (about 52.7B in
both forms) or repeated Boyer CPU time (3.5--3.8 seconds under the current
host load), so it was reverted.  Re-freezing is not the dominant cost in the
hot recursive path; compact Value ownership, dispatch, call frames, and
recursive structural execution remain the measured priorities.
