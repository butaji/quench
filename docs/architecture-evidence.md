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

The current task-073 correction batch has four bounded runtime results. Internal RegExp
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

At `94586e4d01`, leaf and composed publication share one transactional finalized
image carrying exact bytes, identity, ABI and callable-entry offset. A real
prefixed-entry image executes correctly; invalid entry offsets and cache-image
substitution reject before invocation. Layout is 12/12, composition is 8/8,
generated composition is 10/10, and the default runtime is 1000 passed/1 ignored.
This is an AsmJit-inspired boundary reduction, not adoption of its assembler or
a measured speedup.

## Infrastructure

| Contract | Implemented code and normal wiring | Executed evidence |
| --- | --- | --- |
| One canonical declaration | `rust_leaf_catalog!`, `rust_assembly_catalog!`, `RegionRecord` and opcode facts derive IDs, ABI, operations, effects, recipes, holes, links and tests. `PhysicalStencilView` is the single selected code/data/metadata value; `VerifiedRegionImage` carries finalized identity, bytes and callable-entry offset for both leaves and compositions. No C/Clang/runtime LLVM path exists. | Generated+trace runtime 1010/1 ignored; current default runtime 1000/1 ignored; extractor 15/15. Physical-contract mutation, generated/legacy mismatch, nonzero-entry execution and catalog coverage tests pass. The generated ARM64 table contains 55 emitted artifacts. |
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
| Calls/construct (B) | Existing call/construct/frame machinery owns receiver, arguments, `newTarget`, roots and reentry; regions exit before unsupported calls. | Call/constructor/receiver tests and native-to-helper-to-nested-native boundary coverage. |
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
