# Quench stencil JIT: implementation specification

## Status and delivery order

User-authorized implementation contract. This document specifies work; it is not
evidence that the work is complete. MUST denotes a completion requirement.

Implement and verify the infrastructure below before returning to `micros`.
Continue runtime, host, differential, and fault-injection correctness tests during
implementation. After the completion gate passes, resume the frozen micros
workflow for comparative performance and memory evidence. Do not weaken fixtures,
protocols, or thresholds. Preserve concurrent changes and existing working code.

## 1. Scope and invariant

Specialize combinations of runtime facts, not JavaScript syntax or workloads.
Use Quench's existing canonical residual operations and semantic mechanisms.
OXC owns syntax; do not introduce a second AST, semantic IR, runtime crate, or
independent optimizer semantics. `quench-node` remains the host/API boundary.

Full infrastructure means an integrated, reusable system covering every layer
for an explicit supported native subset, with complete ordinary execution for
everything else. It does not mean every ECMAScript feature is implemented in
machine code. Unsupported native cases MUST reject or exit safely, not fail JS.

Family/variant counts and percentage coverage are measurements, not acceptance
targets. There is no requirement for 35 families, 500 variants, or 90% coverage.
Copy-and-patch is runtime compilation with a small backend, not an absence of JIT.

## 2. Canonical declarations and generated consequences

Each supported shape MUST declare or reference canonical facts for:

- Residual operations, operand roles/relationships, flags and target support.
- Required Proven/Guarded/Unknown facts, effects and invalidation dependencies.
- Entry ABI, register inputs/outputs/clobbers, spills and live-out requirements.
- Legal entries, control-flow successors, helpers, roots and exit contracts.
- Template identity, relocations, alignment and bounded resource costs.

Generate mechanical IDs, tables, accessors, operand bindings and validation from
these declarations. Do not duplicate existing semantic facts into this schema.
Use composition and target-specific physical views of the same facts.
Avoid hand-maintained table indices and parallel allowlists.

Scalar leaves, raw-memory kernels, helper bridges and general region entries
MUST have non-interchangeable invocation contracts. Type-safe constructors and
entry wrappers MUST enforce ABI compatibility; opcode matching is insufficient.

Prefer build-time compiler-generated templates where practical. For necessary
assembly use labels and extracted/validated relocation records instead of hand
counted branch offsets. Keep generation reproducible with explicit toolchain
requirements. A build-time LLVM toolchain must not become a runtime dependency.
Retain a small audited target patcher, rejecting unsupported relocations.

## 3. Semantic coverage and execution classes

N = required native core; G = required guarded specialization; B = required
complete boundary/fallback integration. B does not require a native body.
Every row needs ordinary-source regression coverage and a recorded support status.

| Family | Class | Required behavior |
| --- | --- | --- |
| Values/locals | N | Constants, moves, numeric/tagged local transfer; live-outs, TDZ and immutable/deleted bindings preserved |
| Captures | B | Canonical captured-cell access and rooting; use guarded word transfer only where ownership facts permit |
| Arithmetic | N | f64 arithmetic; guarded integer specializations where implemented, never implicit wrapping for JS Number |
| Comparison/bitwise | N | Numeric comparison, guarded same-tag equality, exact JS bitwise conversions and shift rules |
| Control | N | Truthiness/nullish exits, conditions, branches, induction, native backedges and return |
| Own property get/set | G | Ordinary data-slot IC hits with layout, descriptor, receiver and mutation validity |
| Prototype property get | G | Actual chain/dependency validity, absence of shadowing and correct owner/receiver |
| Shape transition set | B | Complete ordinary semantics initially; native admission additionally proves capacity, extensibility, prototype/descriptor restrictions and safe commit |
| Indexed elements | N/B | Native guarded dense numeric loads/stores; holes, sparse/exotic and unsupported typed-memory cases use complete semantics |
| Calls/construct | B | Shared JS/native call and construct paths, correct receiver/newTarget, roots, reentry and exact continuations |
| Objects/closures | B | Shared allocation/initialization/capture semantics; no unrooted intermediate native state |
| Frames/returns | N/B | Region frame entry/live-state mapping and return; reuse existing dynamic call-frame semantics |
| Exceptions | B | Exact fault PC and completion, catch/finally behavior, no replay after effects |
| Iteration | B | Shared iterator protocol and closing; fast array/string iteration optional after facts prove equivalence |
| Async/generators | B | Exit before unsupported suspension; preserve await/yield/resume through ordinary machinery |
| IC lifecycle | G | Existing mono/poly/mega semantics, bounded entries, dependency invalidation and complete miss path |
| Strings/BigInt/exotics | B | Complete conversions, string operations, BigInt rules, proxies, eval/with and other exotic behavior |

Optional specializations MUST use the same infrastructure; their absence does
not excuse missing boundary integration. Accessors are guarded calls, not field
loads. Promise/RegExp/private-field behavior remains in existing shared semantics
unless separately specialized under proven contracts; these are not permanently
classified as cold. Do not add IC machinery parallel to existing quickening.

## 4. Required semantic guard contracts

- Integer inputs do not establish integer results: guard overflow, fractional
  division, division by zero and signed zero. Preserve Number ordering, NaNs and
  infinities. No reassociation or FMA that changes observable arithmetic.
- Shape guards must establish ordinary data layout and descriptor assumptions.
  A prototype's shape alone does not establish its identity or the intervening
  lookup path. Use validated dependencies or correct chain guards/invalidation.
- Stores preserve writable/extensible/receiver/prototype behavior and strict-mode
  errors. Capacity growth may allocate: root state, refresh pointers and apply
  barriers required by Quench's collector before a safe commit. Never expose
  a partially initialized new layout at an observable boundary.
- Holes may require prototype lookup. Typed-memory access additionally respects
  its element conversion, bounds, detachment/resizing and buffer semantics.
- Guard facts may be hoisted only across effects proven not to invalidate them.
  Preserve computed-key evaluation, coercion order and optional-chain short circuit.

## 5. Region verification, native composition and runtime integration

Plan construction MUST use actual existing lowered control flow, including loop
test/body/update fragments where present. Verify legal entries, successors,
operand aliases, flags, constants, liveness, and exception/helper boundaries.
Preserve every live-out register, not only the final result. Never infer dataflow
from adjacent opcodes alone. Static verification belongs before publication.

The required ARM64 demonstration is ordinary JS numeric-array source reaching
normal admission and executing condition, indexed load, arithmetic, store,
induction update and repeated backedge natively. Keep loop-carried values in
machine registers where supported. A per-operation or per-iteration Rust bridge
does not meet this requirement. Entry initialization must not repeat on backedges.

Wire admission into existing normal execution/tiering with cheap Unknown behavior,
bounded specialization and amortized rendering. Explicit test activation is
allowed while default native policy stays conservative. Test constructors alone
do not prove runtime reachability. Preserve existing supported targets and reject
unsupported target/ABI combinations before entry.

## 6. State, helpers, exits and safepoints

Represent these outcomes distinctly through every ABI and caller:

- Rejected before entry: no effects; ordinary execution may start at entry.
- Completed/exited: exact successor/completion and materialized live state.
- Threw: original error, exact fault location and correct semantic continuation.
- Post-entry internal failure: non-retryable; never disguise as pre-entry miss.

Malformed success/status records after entry cannot authorize replay. Helpers
must return exact transitions; do not use region start + 1 for later faults.
Validate the whole shape before effects; dynamic invalidation after helpers must
exit at the correct position. Verify exactly-once effects through outer callers.

At allocating, throwing, reentrant or interruptible boundaries, materialize live
values and roots, respect clobbers, and discard/revalidate invalidatable raw
pointers. Restricted leaf interiors may exclude these effects, but MUST implement
and test safe exits before them. Existing async/generator/finally machinery owns
their semantics. Backedges MUST respect runtime interruption/safepoint policy;
unbounded uninterruptible native execution is not acceptable.

## 7. Executable memory and lifetime

Keep unsafe executable-memory operations behind a small audited boundary.
Distinguish construction, relocation/verification, publication, active use and
retirement. Enforce target write-protection/W^X requirements and instruction-cache
synchronization. Validate ABI layout offsets, relocation range/alignment and
literal addresses before publication. Failure must leave no callable partial code.

Use shared bounded slabs or equivalent allocation sharing, not an executable page
per tiny specialization. Enforce per-owner and aggregate code/metadata/cache/version
budgets. Active entry handles retain the owning allocation; eviction and address
reuse cannot resurrect stale entries. Owner IDs alone do not retain allocations.
Bound admission retries and guard-churn costs. Test allocation failure, publication
failure, active-code retirement and cache disposal using the real ownership path.

## 8. Diagnostics and tests

Optional bounded diagnostics distinguish actual native execution from Rust bridges
and modeled callbacks. Record entries, iterations, guards/exits with PC, rejection
reasons, helper transitions, rendering cost and live/allocated code/cache bytes.
One attempt is not both a miss and a hit. Missing facts are unknown, not zero.
Test witnesses must be invocation-local or isolated from parallel-test interference.
Do not put test-injection branches in production hot paths.

Required verification:

1. Generated catalog consistency and ABI routing, including scalar/region mismatch.
2. Actual emitted ARM64 byte execution and relocation limit/alignment failures.
3. Ordinary-source lowering -> normal admission -> native execution -> exact exit.
4. Zero/one/many iterations, nonzero initial state and interrupt responsiveness.
5. Canonical differential registers/environment/heap/completions/errors from
   identical initial state, including live intermediates and operand aliases.
6. TDZ, deleted/immutable bindings, numeric extremes, holes/prototype accessors,
   mutations, unsupported operands and exact exception/finally handling.
7. Post-effect exit/failure through the outer driver with no duplicated effects.
8. Root survival, allocating/reentrant boundaries or verified pre-helper exits.
9. Resource caps, failures, publication, eviction, ownership and disposal.
10. Existing runtime/host regressions with nonzero relevant test counts; do not
    present cross-compilation or modeled callbacks as executed native coverage.

## 9. Completion gate and handoff

Maintain a matrix with one row per section and coverage family: code locations,
normal-runtime wiring, executed tests/results, supported subset and safe exclusions.
Every mandatory infrastructure contract and cross-layer interface MUST pass.
One working loop is necessary but insufficient. Unimplemented optional native
families require tested ordinary boundaries, not placeholder catalog claims.

If a required item is blocked, report the exact prerequisite/authority needed;
do not silently downgrade the gate. Implement scoped prerequisites and continue.
Do not turn this into another planning-only milestone.

Only after this matrix genuinely passes, resume `quench-bench/micros/README.md`:
correctness first, matched uninstrumented before/after timing and RSS/lifecycle,
related/reserved/composed variants, code/static/cache footprint and V8_v7 checks.
Keep trace evidence separate, reports unique and binary identities exact. Full
qualification requires genuine idle-host conditions. Production-default native
admission needs subsequent evidence; no Bun-win or production-readiness claims
follow merely from infrastructure tests.

## Design background

- [Copy-and-patch compilation](https://arxiv.org/abs/2011.13127): prebuilt binary templates with patched holes.
- [Deegen](https://arxiv.org/abs/2411.11469): specialization, quickening, ICs and generated VM execution mechanisms.
- [CPython PEP 744](https://peps.python.org/pep-0744/): build-time stencil generation and a small runtime compiler.
- [ECMAScript ordinary/exotic object behavior](https://tc39.es/ecma262/2024/multipage/ordinary-and-exotic-objects-behaviours.html): observable get/set/receiver semantics.

These inform the design; they establish neither Quench performance nor a required
stencil count. Repository rules and observable JS behavior remain authoritative.
