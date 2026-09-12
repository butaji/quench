# 385 — Register-resident I32 region connector and physical cover

Status: complete

Make I32 specialization a region context, not another standalone opcode family. A
boundary stencil proves and converts a bounded set of live numeric values once; interior
rustc/LLVM-cooked stencils consume and produce fixed AArch64 `w` registers; side exits
materialize canonical NaN-boxed values once. The typed connector is the category object:

`TaggedFrame -> I32Context<Locations> -> ... -> I32Context<Locations'> -> TaggedFrame`.

The quote remains the existing `RegionPlan`/`RegionOp` data. A pure representation pass
derives candidate I32 islands and liveness. Task 157's physical cover chooses among one
shared kernel, ordinary copied stencils, and a register-resident I32 composite using
static architecture costs for guards, conversions, spills, copied bytes, patches, and
seams. No runtime micro-op interpreter, benchmark identity, hotness counter, or dynamic
LLVM compilation is permitted.

Start with closed straight-line and single-backedge regions containing local loads/stores,
integer literals, bitwise operations, shifts, comparisons, and branches. Use named bounds
for resident values and versions. Addition/subtraction may stay I32 only behind an exact
overflow side exit; unsigned shift has an explicit U32 result context. Dense element
access joins only after element-kind and bounds guards can produce the required context.

Rust macros must generate the finite connector-compatible stencil vocabulary and cooker
audit expectations from one declaration. Composition must remain a normalized free
sequence; the emitted larger region is still a `Stencil<In, Out>` and can be frozen or
shared under the existing Template/Instance/Kernel physical choice.

Preflight evidence: Task 381's isolated bitwise leaves failed at -3.45% because every op
paid F64-to-I32-to-F64 conversion. Task 384 showed that merely making more small stencils
execute can be catastrophically slower than one coarse kernel. Crypto already executes
roughly 1.9 million counted numeric-loop iterations in a 20 ms run. This task must remove
conversion and transfer seams across those successful regions rather than add leaves.

Acceptance: representation/liveness/category-law tests; cooked-stencil disassembly proves
one entry conversion, register-resident interior bitwise operations, and one exit
materialization with no Rust helper; side-exit differential tests cover overflow, NaN,
negative zero, large shift counts, ownership, and branches; counters report guards,
conversions, spills, and selected physical covers; the full randomized nine-pair exact
V8v7 gate improves without a component-floor violation.

## Rejected narrow prototype

A deliberately small implementation trial recognized only
`ReadLocal ; (NumberLiteral ; bitwise-or-shift){2,}` and introduced a nominal I32
connector. It was not the design above: it had no location-polymorphic multi-value
context, liveness, register assignment, spills, overflow exits, or costed physical cover.
Disassembly made the defect concrete. A two-operation chain occupied 368 bytes and 92
instructions; every numeric literal still loaded an F64 `Value` from `InlineSite` and
paid roughly 25 instructions of `ToUint32` conversion. Only three chains containing six
operations were selected in Crypto, and no other suite was reached.

The alternating three-pair, 100 ms screen is recorded at
`reports/task385-i32-literal-chain-ab-3/comparison.txt`:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 994 | 955 | -3.92% |
| DeltaBlue | 966 | 990 | +2.48% |
| Crypto | 1778 | 1860 | +4.61% |
| RayTrace | 1959 | 1820 | -7.10% |
| Earley-Boyer | 3294 | 3305 | +0.33% |
| RegExp | 3498 | 3494 | -0.11% |
| Splay | 3833 | 3924 | +2.37% |
| Navier-Stokes | 6348 | 6383 | +0.55% |
| geometric mean | 2352.37 | 2348.60 | -0.16% |

The candidate violated the component floor on RayTrace and was fully removed. Task 385
remains planned because the general register-resident region connector was not
implemented. The next attempt must first support immediate/raw-value patch bindings and
must select an entire context-carrying region whose entry conversion and exit
materialization are amortized. Merely assigning a new phantom context to ordinary
frame-based leaves is not evidence of physical composition.

## Task 407 connector preflight

A second bounded preflight asked whether the existing frame-based region could first gain
enough from local forwarding or operand-burned bitwise leaves to justify extending that
physical form. It could not. Loop-local forwarding found only three to nine static load
opportunities and the bundled candidate regressed Navier-Stokes 43.77%. Six general
burned-operand bitwise/shift leaves produced no Crypto gain and regressed aggregate 1.71%.
Both prototypes were removed; Task 407 records the raw evidence.

This narrows the implementation boundary. The next Task 385 candidate must introduce the
physical context itself—register locations, live-value assignment, entry conversion,
connector-compatible interior stencils, edge parallel copies, and exit materialization—in
one coherent slice. Rewriting or specializing frame-resident leaves before that context
exists is no longer an admissible experiment.

## Current implementation audit

The first general physical-context scaffolding now exists in source:

- `RegisterRegionPlan` performs a bytecode-derived four-lane linear scan;
- Rust macros cook a finite family of six-argument register-region templates;
- `RegisterRegionConnector` names a distinct categorical boundary;
- the AOT extractor recognizes register-region next, branch, slow, and leave holes.

This is retained work in progress, not a completed or benchmarked optimization. Before the
link-wiring slice below, a release build warned that `NumericRegionLink::register_plan` was
unread and that the new connector types were unused, so no linked V8v7 function could enter
the new family.

More importantly, the cooked lanes currently contain `u64` IEEE-754 bit patterns. The
bitwise/shift leaves call `js_i32`/`js_u32` on every operation and every producer writes the
canonical virtual-register slot through. That is a raw-F64 cache, not the task's required
unboxed I32 context, and it preserves both conversion and frame-store seams. It must not be
described as an I32 implementation or used as evidence that register stencils do not help.

The next slice must correct the representation before measuring:

1. derive one typed location fact per SSA value:
   `TaggedWord | I32(Gpr) | U32(Gpr) | F64(Fpr) | FrameSlot`;
2. fuse `load + guard + unbox` at region entry and memory loads, following SpiderMonkey's
   load-with-unbox fold;
3. keep Word32 chains in AArch64 `w` registers and F64 chains in `d` registers, with no
   per-operation cross-bank conversion;
4. treat slow/deopt frame values as cold reconstruction uses, not reasons for hot-path
   write-through; Task 330 owns the reconstruction recipe;
5. lower CFG merges to parallel-copy edge stencils and rebox only at an observing exit;
6. wire the plan into `numeric_region_stencil`, prove selection with counters and
   disassembly, then run correctness and alternating A/B.

## Link-wiring update

The `RegisterRegionPlan` is now consumed by `numeric_region_stencil`: a successful plan
lowers to a typed `Connector -> RegisterRegionConnector` entry, a sequence of
`RegisterRegionConnector -> RegisterRegionConnector` leaves, and one labeled
`RegisterRegionConnector -> Connector` exit. External control-flow targets other than the
single declared region exit are rejected by the physical planner, so the typed exit cannot
silently jump into a normal connector with unreconstructed state. The release build passes
`cargo check --release`.

This closes the previous *unwired-plan* defect only. It does not satisfy the task's physical
acceptance condition: the connected leaves still carry IEEE-754 payload bits in `u64`
arguments, reconvert bitwise operands per operation, and publish canonical registers inside
the region. Correctness tests, native selection counters, disassembly proof, and V8v7 A/B
remain pending until the connector is representation-typed and edge-materialized as listed
above. No score claim is attached to this wiring step.

The representation variants remain one finite macro-generated catalog, and the quote is
still the single `RegionPlan`. There is no runtime compiler, interpreter fallback, hotness
counter, benchmark identity, or benchmark-shaped superinstruction.

## Accepted mixed Word32/F64 implementation

The retained implementation replaces the misleading raw-`u64` cache with one fixed
representation-typed ABI: four AArch64 Word32 lanes (`w2..w5`) and four F64 lanes
(`d0..d3`). `RegisterLocation` records `Word32<Signed|Unsigned>` or `F64`; explicit
conversion steps are the only cross-bank arrows. All leaves accept and return the same
connector, end in a must-tail continuation hole, and are generated as finite lane matrices
from semantic Rust macros. The build script enumerates names and lane tuples; it does not
duplicate operation semantics.

The physical planner now performs consumer-demand projection. Word-only numeric literals
are copy-patched directly into `w` registers through audited two-lane `MOVZ/MOVK` holes;
local and dense numeric producers fold conversion into their load stencil. Dataflow-proven
store-to-local reads become identity steps and preserve the representation selected by the
store. Slow dense edges reconstruct their canonical index/source operands before entering
the total semantic stencil/kernel. No producer writes a canonical virtual register on the
successful path.

Cooked-object evidence from
`target/release/build/deegen-23a41bd900f97f93/out/stencil_handlers.o` includes:

- `deegen_register_region_bit_xor_w012`: `eor w2, w4, w3` plus the patched tail;
- `deegen_register_region_add_d012`: `fadd d0, d1, d2` plus the patched tail;
- `deegen_register_region_signed_word_to_f64_d0w1`: one `scvtf d0, w3`;
- `deegen_register_region_load_word_literal_w0`: two audited patchable `mov` instructions;
- `deegen_register_region_store_local_s0`: canonicalization/materialization only at the
  observing store edge.

`cargo test --release` passes **159 tests**. A Crypto stats run selected five register
regions with 36 explicit conversions, 24 forwarded local reads, 23 Word32 literals, 20
Word32 folded loads, and zero spills. In the principal Crypto loop, representation demand
reduced the plan from the old 19 conversions/86 steps to 10 conversions/77 steps; smaller
loops fell from eight conversions to two.

The required randomized nine-pair exact gate is retained at
`reports/task385-mixed-register-exact-9/comparison.md`. It passed with aggregate geometric
mean **2501.00 vs 2390.53 (+4.62%, 95% paired bootstrap CI +3.57% to +5.57%)**. Crypto
improved **+37.04%**; all other suite changes remained inside the component floor. The
accepted binary is `/tmp/deegen-task385-mixed-register-candidate`, SHA-256
`df3340936531851f29ef9638966b627d487abf1586dacf77843e3549d43f56ce`.

Additional primary sources: V8 Maglev's representation selection and forward register
allocator <https://v8.dev/blog/maglev>; SpiderMonkey's `FoldLoadsWithUnbox`, effective
address analysis, late edge-case analysis, and scheduling passes
<https://firefox-source-docs.mozilla.org/js/MIR-optimizations/index.html>; CPython 3.15's
register-cached copy-and-patch variants
<https://github.com/python/cpython/issues/135379>.
