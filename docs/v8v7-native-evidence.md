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
