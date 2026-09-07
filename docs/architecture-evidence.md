# Task 075 — current completion matrix

Status: **gate PASSED at `ecb00540c5`**. Subsequent default-runtime correctness
at `57e428e176` is 996 passed/1 ignored and host correctness is 16/16. The
current complete benchmark baseline is `b00f1961b0`; its production binary is
SHA-256 `88f06a8d46f029e2d825663ac960f19ec1234d5f09e80e3716be14f7158117a2`.
Generated-object configuration must still be rerun at the final task-073 revision.
Task 073 may now measure the frozen corpus; passing this gate is not a speedup or
production-default claim.

Current uninstrumented evidence: 372/372 all-size timing scenarios and 844/844
reserved/all-size/legacy smoke scenarios pass. Three complete V8_v7 repetitions
produce a median-index geomean of 73.83 (MAD 0.20). These are correctness and
prioritization observations; lifecycle lanes and final generated-object
validation remain open in task 073.

The current task-073 correction batch has five bounded runtime results. Internal RegExp
descriptor reuse is semantically valid but performance-neutral. Mapping an
observed intrinsic Array species to the existing fresh ordinary-array path
removes per-element replacement/descriptor work: an alternating 64x1024 slice
control improves from 1.61--1.70 s/~87.4 MB to 0.03 s/~43.3 MB while preserving
custom-species ordering. A name-first internal own-property fact improves the
controlled ordinary write lanes by roughly 16--21%; its rejected generic
projection was 8--9x slower because it decoded unrelated values. Reusing the
existing identity-preserving backing for current dense numeric `Reflect.set`
writes improves a 100,000-write control from 211--218 ms to 199--200 ms. Separate
Crypto samples remain 15.9--16.2, so none of these results is credited as a
Crypto or aggregate improvement.

At `da32350972`, an ordinary-source 19-op affine integer loop reaches a generated
ARM64 region with one entry, 30 native backedge iterations, and no per-iteration
Rust dispatch. Before the wiring repair the same site recorded 4,095 misses and
zero hits. A different-name/different-constant clone improves from a 555 ms
median with native regions disabled to 3 ms in composed mode (Node 11 ms, Bun
2 ms). Frozen `calls/inline` comparisons improve by 80.0%, 96.65%, and 99.283%
at small/medium/large sizes, while remaining 158.2x, 12.2x, and 2.30x behind Bun.
This isolates the remaining cost to cold admission and surrounding call/frame/
host work rather than the resident ARM64 loop body. Default ARM admission stays
off. Generated-object tests pass 1,021/1 ignored; default tests pass 1,008/1
ignored. The AArch64 catalog now derives every emitted instruction from named
opcode/condition/register-aware encoders; the refactor caught and fixed a `w0`
versus `w1` destination error that the non-generated comparison tests exposed.

At `75506ca125`, completed call frames release both their execution guard and
cycle-collector root before attempting pool admission. On the frozen
`calls/direct` small/medium/large diagnostic, environment allocations change
from 2,616/17,400/135,672 to a flat 398/398/398 while all three results still
match Node. Retired instructions remain effectively flat (within about 0.9%,
with the large case improving about 0.06%), so allocation removal is real but
is not the dominant throughput lever. The paired uninstrumented lane remains
1,889x/972x/898x slower than Bun. The next call work must remove dispatch,
frame initialization and operand traffic across useful guarded callees rather
than tune the allocator or native arithmetic leaf.

At `6e6cb844a0`, the call IC and cold-call gateway consume the same immutable
physical-body fact instead of the hot `execute_direct` path bypassing it.  A
bounded named-method affine loop derived from the ordinary 28-op CFG validates
own data slots and a pure affine callee once, then executes the loop without
per-iteration property, call, or residual dispatch.  On `calls/direct`, the
Quench/Bun ratio changes from roughly 838x/404x/370x to
115x/12.8x/5.7x at small/medium/large sizes.  A matching trace records 33
`PrecompiledAffineNamedLoop` entries and reduces compact handlers from about
1.28 million to 7,181.  All 18 development and 36 reserved call scenarios are
correct.  This is currently a Rust precompiled region, not a generated native
body; the remaining gap and native routing are open rather than credited as a
completed stencil win.  Evidence:
`target/micros/calls-{direct,all}-hot-specialization-measure-*.json` and
`target/micros/calls-direct-hot-specialization-diagnostic-1788815600.json`.

The next build-policy ablation found that `composed` execution on an ordinary
production artifact remained ~313x Bun for `calls/inline`; the identical tree
built with Rust object generation was 2.27x.  Supported-target builds now emit
the verified Rust artifact table by default, while
`QUENCH_DISABLE_STENCIL_OBJECTS=1` retains an explicit empty-artifact fallback
test configuration.  This changes availability, not runtime admission: ARM64
composed execution remains policy-disabled until broader measurements justify
it.  Generated bodies pass 32/32 focused tests and 1,031/1 ignored full runtime;
the disabled selector boundary passes 4/4.  Evidence:
`target/micros/calls-inline-{composed,generated-composed}-*-measure-*.json`.

The aggregate `composed` switch also hid three distinct generated ABIs. The
runtime policy now derives kernel, numeric-array-loop and affine-loop admission
independently from the canonical `RegionAbi`; `composed` remains only their
diagnostic compatibility bundle. On one identified generated production binary
(SHA-256 `e193fad8b09e2aab54b0fd62a5f8800441d89b8272c3dfb55fb084c542503211`),
large `calls/inline` is 326x Bun with default policy, 324x with kernels only,
322x with the array loop only, and 2.23x with the affine loop only. The affine
trace records 33 actual region entries, 135,166 native backedges, zero misses,
84 used code bytes and one 4 KiB slab. All 36 reserved call cases and 18 numeric
cases pass. A paired one-sample V8_v7 sweep is neutral within host variation:
the numeric-cluster geomean is 108.87 default versus 109.34 affine-only, while
the other-five geomean is 88.78 versus 88.01. Therefore the split is retained
as a causal diagnostic correction and ARM default admission remains unchanged.
Evidence: `target/micros/*policy*-efbd45cc91-dirty.json`,
`target/micros/calls-inline-affine-loop-diagnostic-efbd45cc91-dirty.json`, and
`target/v8-policy-*-efbd45cc91-dirty-run1.json`.

A corpus-ledger follow-up found a separate admission-order defect in the
kernel lane. `numeric/floating/large` reached 11 array-region sites and no
native entry, but dynamic guards ran only after publication, leaving 320 used
bytes, 16 cache rows and 36 KiB of executable mappings. Physical publication
now occurs inside the invocation edge, after dynamic admission; pre-entry
publication/lease failures remain retryable while malformed post-entry status
remains committed. The identical diagnostic now reports zero used, resident,
cached and process-executable bytes at all 11 rejected sites. The focused tests
and full runtime pass (1,031/1 ignored). On production binary SHA-256
`fffc242d0c5faa8bf1ddbe4c285ec35952c72ba4cf310bab086cb0e9311e2b85`,
kernel mode is still about 4.7% slower than default on that case (246.14x
versus 235.20x Bun), so repeated failed admission—not rendering—is the next
measured design gap. Evidence:
`target/micros/numeric-floating-kernels-diagnostic-{fdb1c15963,lazy-region}.json`
and `target/micros/numeric-floating-{default,kernels}-lazy-region.json`.

RayTrace then exposed an independent correctness boundary in local fusion. Its
trace is dominated by property/control/local windows (94,850,442 compact
handlers; zero default stencil entries), while exact catalog region sequences
do not match those mixed CFG blocks. Family-isolated runs were valid for numeric
fusion (score 102) and predicate fusion (105), while property fusion initially
produced an incorrect scene. Four leaf/commit ablations did not help. A synthetic
clone then showed correct direct values but a missing value in later aggregate
construction: the property window had admitted an unrelated producer outside
the receiver dependency cone and skipped its residual load. Requiring a connected
cover fixes both the clone and unchanged RayTrace; property-inclusive fusion is
valid at 100. This is a correctness win, not a speedup: it remains about 1,441x
behind Bun and slightly trails the property-excluded 104 sample. The next work is
bounded residual-CFG machine-pattern tiling with delayed register roles, not more
exact opcode-sequence kernels. Evidence:
`target/v8-raytrace-{fusion-numeric,fusion-property,fusion-predicate}-dirty-run1.json`
`target/v8-raytrace-{safe-fusion,connected-property}-dirty-run1.json`.

At `94586e4d01`, leaf and composed publication share one transactional finalized
image carrying exact bytes, identity, ABI and callable-entry offset. A real
prefixed-entry image executes correctly; invalid entry offsets and cache-image
substitution reject before invocation. Layout is 12/12, composition is 8/8,
generated composition is 10/10, and the default runtime is 1000 passed/1 ignored.
This is an AsmJit-inspired boundary reduction, not adoption of its assembler or
a measured speedup.

At `52178bbc90`, that staged boundary also owns the common typed scalar
render/publish/invoke/cache transition, and `0e36f1432e` reduces the two idle
eviction APIs to one generation-aware owner transition. Typed callable
conversion, F64 fallthrough permission and semantic fallback remain explicit.
Focused lifecycle tests pass and the default runtime is 1000 passed/1 ignored.

## Infrastructure

| Contract | Implemented code and normal wiring | Executed evidence |
| --- | --- | --- |
| One canonical declaration | `rust_leaf_catalog!`, `rust_assembly_catalog!`, `RegionRecord` and opcode facts derive IDs, ABI, operations, effects, recipes, holes, links and tests. `PhysicalStencilView` is the single selected code/data/metadata value; `VerifiedRegionImage` carries finalized identity, bytes and callable-entry offset for both leaves and compositions. Supported-target builds generate this Rust artifact table by default; an explicit disable configuration tests empty-artifact fallback. No C/Clang/runtime LLVM path exists. | Generated bodies 32/32; full generated runtime 1031/1 ignored; disabled selector 4/4; extractor 15/15. Physical-contract mutation, generated/legacy mismatch, nonzero-entry execution and catalog coverage tests pass. |
| Bounded CFG planning | `stencil_cfg`, `stencil_binding`, `stencil_value_graph`, `stencil_plan`, `stencil_region_links` and the region builders consume existing lowered PCs and derive predecessors, use/def, liveness, aliases, effects, legal entries/exits and symbolic transfers. Folding, value numbering, dead-pure elimination and fusion are disposable bounded selection data, not another semantic IR. | CFG/admission and architecture-invariant tests pass in all runtime configurations. Ordinary-source arithmetic chains, comparison branches, Boolean control and numeric-array loops reach their declared physical images; broken facts remain ordinary. |
| Typed ABI and continuation | Canonical `RegionAbi`/`ContinuationAbi` facts select closed entry wrappers. `EntryToken` is non-owning; `AllocationLease` retains the exact published generation and releases pool borrows before invocation. `PhysicalInstallation<I>` is the single plan storage/cache/lifecycle authority. Generic caller-supplied render/execute APIs were removed. | ABI crossing, pointer/address forgery, stale generation, reentry, nested native invocation and retirement tests pass. Focused arena tests: 56/56. |
| Relocation and publication | Rust object extraction preserves exact symbol ranges, code/data and declared relocations. `stencil_layout` resolves typed Branch26/Branch19/Rel32 and literal holes transactionally. Publication records exact identity before one W^X/cache-flush transition; a composed cache signature locates a candidate but exact finalized bytes authorize reuse. Unsupported relocation/control forms reject. | Extractor 15/15; signed limits, alignment, addends, reordered/missing/duplicate holes, transactional failure, wrong ABI, cache relabel/collision, and actual ARM64 successor/backedge execution pass. Current default runtime: 991/1 ignored; generated-object runtime: 1002/1 ignored. |
| State, roots and exits | Native outcomes preserve pre-entry rejection, exact completed transition, throw PC and non-retryable committed failure through the ABI and outer driver. Helper/reentry boundaries materialize state and roots; allocating/reentrant native interiors are rejected before entry. Native loop backedges poll the runtime interrupt flag. | Bridge/helper/reentry/root tests, exact throw/finally/no-replay tests, interrupt-after-committed-iteration and active/suspended root tests pass in the full runtime suite. |
| Bounded ownership and diagnostics | Shared slabs pack small entries; per-owner and process budgets charge mappings, cache metadata and retired-live generations. Active leases delay reclamation; retirement blocks new admission; cache rows cannot resurrect reused addresses. Optional witnesses distinguish emitted native execution from bridges and modeled callbacks. | Stable-hit/no-rerender, cold-unknown/no-allocation, slab sharing, aggregate exhaustion, active retirement, independent idle reclamation and disposal-baseline tests pass. Generated execution-trace runtime: 1005/1 ignored. |
| Cohesive boundaries | Arena mapping, rendering, execution, pooling, typed entries and accounting are separate modules, each below 500 handwritten lines; tests are split by contract. The arena production parent is 375 lines and its six responsibility modules are 148–448 lines. | Default runtime 983/1 ignored; generated runtime 994/1 ignored; generated+trace 1010/1 ignored; `quench-node` 16/16; full workspace green at `ecb00540c5`. |

## Semantic families

| Family/class | Supported implementation and complete boundary | Normal-path and hostile evidence |
| --- | --- | --- |
| Values/locals (N) | Generated constants, numeric/tagged moves, load/store locals and exact live-out materialization. TDZ/deleted/immutable cases reject before entry. | Ordinary-source constant/local/move/load/store tests; owned tagged identity and immutable-store rejection. |
| Captures (B) | Existing cells/environments remain semantic authority; native code exits before unsupported capture/helper work and frame roots retain live cells. | Shared-cell identity, allocating/reentrant bridge, suspended capture survival and released-cycle tests. |
| Arithmetic (N) | ARM64 Number add/sub/mul/div/negate/increment plus compatible linked fragments; no reassociation/FMA or unchecked integer result assumption. | Actual-byte and ordinary-source tests include signed zero, NaN/infinity, coercion guard misses and heterogeneous chains. |
| Comparison/bitwise (N) | Numeric comparisons, tagged identity equality, ToInt32 bitwise/not/shifts and UInt32 result representation. | Actual-byte and ordinary-source tests cover NaN ordering, fractions, range conversion, masked shifts and coercion fallback. |
| Control (N) | Truthiness/nullish, comparison/Boolean branches, returns and interruptible ARM64 induction/backedges. | Generated three-fragment true/false composition and ordinary-source zero/one/many numeric-array iterations with nonzero initial state. |
| Own property get/set (G) | Native receiver/layout/descriptor/slot guards perform the word load or existing-slot commit; misses stay in the ordinary property mechanism. | Ordinary-source read/write, stable mono/poly reuse, accessor/descriptor mutation, non-writable and strict-store controls. |
| Prototype property get (G) | Native guarded prototype-owner slot read validates receiver, owner/layout and absence of invalidating shadowing. | Ordinary-source prototype hit plus shadowing, accessor and prototype-mutation fallback tests. |
| Shape transition set (B) | Capacity/layout-changing stores remain ordinary; native store handles only proven existing writable data slots. | Creation order, extensibility, descriptor, receiver and strict failure tests. |
| Indexed elements (N/B) | Guarded dense f64 load/store/update and the composed numeric loop are native; holes, sparse/prototype and unsupported typed-memory cases remain ordinary. | Actual ARM64 array block/loop, aliases, bounds, holes, inherited access and mutation controls. |
| Calls/construct (B/G) | Existing call/construct/frame machinery owns receiver, arguments, `newTarget`, roots and reentry. A guarded precompiled region consumes the existing pure affine-callee fact; unsupported calls remain exact boundaries. | Call/constructor/receiver tests, hot-IC physical-body reuse, guarded named-loop clones and native-to-helper-to-nested-native boundary coverage. |
| Objects/closures (B) | Existing allocation/capture semantics and code-owner leases remain authoritative; no unrooted native object intermediate is supported. | Closure/code-store lifetime, collection, repeated compilation and final-owner drop tests. |
| Frames/returns (N/B) | Frozen operand-role frame widths, same-frame structured fragments and exact region live-outs; dynamic calls retain canonical frames. | Zero/one/many argument windows, absent-argument sentinel, nested callee width, OSR retirement and return tests. |
| Exceptions (B) | Exact fault PC, committed state and canonical catch/finally paths; no generated-frame unwind. | Effect-then-throw, allocating finalizer, malformed status and exactly-once tests. |
| Iteration (B) | Iterator protocol/close stays ordinary; native loops cover only declared dense numeric regions. Structured suspension records the executed operation plus explicit loop, try and iterator frames; repeated async suspension updates rather than duplicates an active frame. | Continuation contract 11/11, default runtime 983/1 ignored, host 16/16. Production CLI matches Node for conditional/sequential await (`21`, `[[1,1],[2,2]]`) and the former legacy generator shapes (`[1904,64]`, `[7392,64]`). |
| Async/generators (B) | Native admission exits before suspension; one `SuspensionPoint` stack owns executed PC/destination and nested loop/try phases. | Continuation contracts and Node-facing tests cover nested/sequential await, generator progress/return/finally and captured-root collection. |
| IC lifecycle (G) | Existing mono/poly/mega quickening facts drive property variants and bounded retirement; no parallel IC universe. | Stable alternatives reuse code, incompatible facts do not alias, mutation invalidates, megamorphic sites degrade safely. |
| Strings/BigInt/exotics (B) | Existing ordinary conversions and semantics remain the complete path; no native proxy/eval/with/string/BigInt interior is claimed. | Full runtime/host regressions plus string surrogate and ordinary coercion/error tests. |

## Completion coherence review

Reviewed end to end: OXC lowering and residual ownership; opcode/effect/register
facts; values, roots and heap ownership; object layouts and IC dependencies;
calls, frames and structured suspension; interpreter/native adapters; Rust-only
artifact generation; relocation/publication/leases; Node output/event-loop
boundaries; and executable Cargo tests. Unrelated Node module implementations and
the Wasm engine were regression-tested, not exhaustively architecture-audited.

The retained tasks are coherent: 075 supplies the verified bounded native
mechanism; 073 owns measurement and evidence-led profitability; 060 remains an
unapproved allocation proposal; 076–085 are independent cleanup/simplification
work and do not redefine stencil semantics. The AsmJit comparison contributed
its code-holder/finalization/publication separation and removal of a duplicated
generated byte field. Quench keeps Rust catalog and object artifacts instead of
importing an assembler/compiler/runtime. Deegen's
generated contracts and Sparkplug's compatible-frame/direct-control principles
fit; a second optimizer IR, global register allocator, runtime LLVM, SIMD quota
or arbitrary stencil-count target does not.

Representative generated ARM64 inspection remains deliberately structural:
the Number add leaf is `fadd` plus `ret`; a linked fragment is `fadd` plus a
declared successor branch; the loop has one-time initialization and a native
backedge. Tests assert semantic results and contracts, not compiler-version byte
counts.

Safe exclusions are explicit: unsupported target/ABI/relocation combinations,
allocating or reentrant native interiors, proxies/accessors, sparse or unsupported
typed-memory arrays, exotic coercions and the unverified AArch64 optimizing
driver take complete ordinary semantics. A current task-073 ablation found that
the former aggregate ARM development opt-in could corrupt structured numeric
loops; the policy now exposes only independently selectable verified leaves and
composed regions on that target. Both are still default-off and currently
unprofitable in the measured RayTrace/NavierStokes pair.
Production ARM admission stays conservative until task-073 measurements; passing
this gate establishes correctness and boundedness, not a speedup or Bun parity.
