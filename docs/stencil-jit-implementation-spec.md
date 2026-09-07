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

Treat interpreter, generic native, IC-specialized, fused and loop-region execution
as capabilities over shared semantics, not five mandatory independent tier states.
No new quota of250–400 families,3,000–8,000 variants or100–300 fusions is required.
Generate structural variants only when representation, ABI, effects or instruction
encoding changes. Property offsets/constants are typed patch bindings unless their
range or encoding forces another form; reject or select a declared alternative.
Prune impossible/dominated combinations instead of enumerating every axis product.
Budget shipped template bytes, matcher metadata, resident pages and emitted hot
code separately: unused template data does not directly occupy instruction cache.
Rank valid fusion candidates with bounded cost estimates for bytes, moves/spills,
guards/exits and removed dispatch; longest match alone is not a profitability proof.
Derive loop plans from existing control flow and Proven/Guarded facts, with explicit
alias/effect kills, liveness, safe entry/exit maps and bounded work/code budgets.
Invariant guards require valid inputs at entry, not merely absence of local stores;
callbacks, prototype/shape mutation, resizing and reentry can invalidate assumptions.
Runtime planning may compose offline-optimized Rust bodies but cannot assume LLVM
optimizes across patched edges or that stable Rust guarantees tail-call/register ABI.

## 2. Canonical declarations and generated consequences

Apply the `lisp-mindset` skill throughout this design, using idiomatic Rust:
model facts first, derive consequences, compose small mechanisms and keep effects
at explicit boundaries. Extend the existing Rust macro catalogs rather than
creating a parallel stencil DSL with its own semantic facts. Prefer declarative
Rust macros for repetitive declarations, typed wrappers, tables and test cases;
macros must not hide exceptional JS behavior or unsafe execution contracts.
Build-time assembly/compiler tooling remains responsible for machine bytes.

Represent lifecycle and execution outcomes as typed state transitions. Derived
views must not become independently mutable authorities. Avoid boolean-only
contracts that merely restate their own labels without validating execution.
No macro-per-case expansion, string-replacement framework or opaque abstraction
should replace a clear shared declaration and explicit irreducible behavior.

Apply the skill's size limits to the stencil implementation and its refactored
integration units: functions at most 40 lines, complexity at most 10, source
files at most 500 lines. Split by cohesive responsibility, not arbitrary chunks
or one-use forwarding wrappers. Do not conceal oversized handwritten code inside
macro token bodies. Generated artifacts remain derived, not hand-maintained.
Preserve concurrent work; avoid unrelated repository-wide refactoring.

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

### Selected implementation architecture

Use a bounded copy-and-patch backend over existing residual operations, not a
second optimizing VM. This is the best-fit design decision for Quench's current
constraints, not a claim that it outperforms every alternative.

The dependency direction is:

```text
existing semantic facts + existing lowered CFG
  -> verified physical region plan
  -> target stencil recipes + checked relocations
  -> published typed entry + allocation lease
  -> exact VM continuation
```

The physical plan references existing operation IDs, values and PCs; it records
only bindings, guards, native layout, roots and exits. It is not another semantic
IR. Unknown facts select ordinary semantics cheaply. Existing quickening owns
observations and invalidation; the backend consumes them without a second IC.

Keep these responsibilities separate and compose them:

- **Catalog:** extend the existing declarations to reference semantic facts and
  declare physical signatures, templates, effects and patch slots once. Generate
  closed ABI routing, constructors, validation tables and repetitive test cases.
  Explicit target templates and exceptional JS behavior remain inspectable.
- **Planner/verifier:** pure bounded selection and placement over the actual CFG.
  Compose compatible core stencils into straight-line blocks and bounded regions,
  with explicit branch targets, live-outs and a finite register/spill convention.
  Reject incompatible compositions before publication. Do not substitute a
  growing list of whole-loop opcode patterns for reusable composition. Fused
  kernels are optional physical recipes using the same contracts and budget.
- **Emitter:** copy build-time artifacts and resolve typed patch records. Prefer
  compiler-generated templates; retain audited assembler/label-based templates
  where required by the ABI. No runtime LLVM, ad hoc optimizer framework, or
  independent handwritten semantic implementation in the generator.
- **Code store:** own mappings, publication, leases, retirement and budget charges.
  Plans and caches reference capabilities; they do not each own executable pages.
- **VM adapter:** materialize canonical live state, acquire the entry lease,
  invoke through its exact ABI, and interpret one typed outcome. Reuse ordinary
  helpers and completion handling. Keep unsafe conversion and effects here or
  inside the code-store boundary, not spread through opcode dispatch.

AsmJit's useful model here is its `CodeHolder`, not its runtime assembler API:
one target-bound value carries sections, labels, relocations and entry offsets;
emitters populate it, then flattening and relocation precede one allocation and
publication step. Quench applies that shape with `PhysicalStencilView` and a
finalized `VerifiedRegionImage`. Selection derives one immutable
`RegionImageIdentity` (key, patched physical signature and ABI); composition
adds finalized bytes, and publication consumes only that image. The arena cannot
reconstruct or relabel identity from parallel arguments. Typed labels and fixups
remain bounded Rust data and resolution stays transactional. Do not import
AsmJit's instruction builder, compiler, allocator or runtime dependency: those
would duplicate the canonical Rust catalog and the existing slab/lease lifecycle.
The first reusable builder deliberately accepts only repeated fragments sharing
one declared internal register convention. Equal external ABIs alone do not make
two arbitrary function bodies composable; additional fragment roles must be
declared and verified before the builder may mix them.

### Bounded stencil-selection optimizer

Optimize selection from the finite stencil catalog, not arbitrary machine code.
Before layout, build a disposable basic-block view from canonical PCs, operand
roles, use/def, effects and `Proven`/`Guarded` facts. It may record value links
and selected representations, but it MUST NOT own semantics, survive publication
or become a second SSA/IR. Bound its nodes, edges, iterations and candidate probes;
budget exhaustion selects ordinary execution.

Apply transformations only when the existing facts prove them, in this order:

1. Specialize known shapes, slots, constants, element layouts and addresses into
   typed patch bindings or a declared specialized recipe. This is the primary
   value lever because it removes generic semantic work and repeated guards.
2. Propagate constants and eliminate pure dead results before copying fragments.
   Never discard coercions, throws, stores, calls, allocation or invalidations.
3. Use local value numbering within one effect-safe block to reuse guards,
   addresses and equivalent pure values. Each relevant effect kills its facts.
4. Select target addressing/strength-reduced recipes and resolve a small fixed
   register-role vocabulary. This is bounded placement, not global allocation.
5. Rank declared fusions by removed dispatch, guards, boxing and transfers minus
   moves, spills, exits and bytes. A longer match is not automatically better.
6. Lay out the expected hot successor inline and cold exits out of line. Invert
   a branch only when its condition and exact continuation remain unchanged.
7. After relocation, run a target-specific fail-closed peephole allowlist for
   identity moves, redundant proven loads/stores and jump-to-next. Recompute or
   reject affected relocation/range metadata transactionally; never rewrite an
   unrecognized instruction stream.

LLVM optimizes each build-time recipe before extraction, but not across patched
fragments. Cross-fragment value retention therefore comes only from the declared
continuation ABI, register roles and verifier. Unit tests MUST compare each
transformation with independent canonical execution and a fact-breaking fallback,
and deterministic counters MUST show the intended work was actually removed.

An entry MUST be constructed from a verified published region and a closed ABI
identity, never an arbitrary address paired with a caller-supplied function
pointer. Private fields and restricted constructors enforce this. Checking that
an address belongs to an executable slab does not establish its entry boundary,
signature or correspondence to the invoked pointer.

Use one authoritative physical installation payload integrated with the existing
`StencilLifecycle`; do not introduce a competing lifecycle. Represent legitimate
ABI alternatives with a bounded discriminated union, not parallel optional raw
and shared entries plus independently mutable result-kind flags. Common retirement
clears the whole installation atomically at the logical level; semantic exceptions
must not be misclassified as physical invalidation. Cache records are disposable
derived indices, never a second authority for callability.

Separate an allocation-retaining lease from any non-owning lookup token. Acquire
the lease before invocation and release mutable pool/cache borrows before a helper
can reenter. Retirement prevents new admission while active leases retain code and
associated metadata through return or unwind. Never reopen an actively executing
slab for patching: publish a replacement or use independently synchronized data
slots under an explicit contract. Count retired-but-live allocations against the
aggregate budget; budget exhaustion rejects admission instead of forcing reclaim.

Migrate incrementally, retaining correctness after each slice: finish the existing
constant/move slice, then migrate binary, unary, truthiness/nullish, property and
region paths to the same mechanism. Remove superseded fields and cleanup paths
in each slice. Do not retain two production architectures after migration.
Split catalog/generation, target templates, verified plans, code ownership and
VM adapters into cohesive modules under the skill's limits; no unrelated rewrite.

### rustc/LLVM artifact pipeline

User-selected implementation: Rust-owned generation using rustc/LLVM, without
C templates or a Clang dependency. This supersedes the previous Clang direction.
Keep the VM, helpers, catalog, object extractor and runtime patcher in Rust.
Compile suitable whole-function Rust leaves/kernels with `rustc --emit=obj`.
For composable interiors needing exact registers and continuations, generate
labeled target assembly through Rust `global_asm!`; use `naked_asm!` only where
its whole-function contract fits. Assembly remains target-specific assembly,
not ordinary Rust optimized by LLVM. Derive bindings/layout constants from the
same Rust catalog and validate compiled artifacts against it. No new runtime crate.

This choice borrows CPython's object-extraction approach, not its separate
micro-op architecture or C toolchain. Do not add rustc-private dependencies,
rewrite rustc's LLVM IR text, or introduce a parallel LLVM-IR generator. Do not
require nightly explicit tail calls or rely on incidental tail-call optimization.
Use explicit assembly continuation transfers for the stable-toolchain path.

1. Record target triple, CPU/features, compiler identity/version, template/catalog
   hash, physical ABI version, layout constants and generation flags in an
   artifact fingerprint. Use the Cargo target, not the build host. Probe required
   rustc target/features and record its LLVM version. Invoke the configured Rust
   compiler for isolated build artifacts without recursively building the runtime.
2. Compile isolated templates to relocatable objects, with no cross-template LTO,
   fast-math, implicit runtime instrumentation or uncontrolled helper dependencies.
   Use named external hole/continuation symbols and explicit entry symbols. Do not
   depend on symbol adjacency, sentinel byte searches, or optimization luck to
   discover function boundaries or preserve patch sites.
3. Parse object symbols, sections and relocations (Mach-O first for the current
   ARM64 host; preserve existing target support). Extract code and all referenced
   literals/data; classify every external reference as a declared hole, helper,
   continuation or explicitly supported relocation. Reject anything else. Verify
   no unexpected TLS, unwind/exception dependency, stack protector or compiler
   helper escaped into a supposedly leaf template.
4. Generate immutable Rust artifact tables with entry bounds, bytes, data, typed
   relocations and ABI/effect metadata. Artifact mismatch fails regeneration or
   disables optional native admission explicitly; never load stale compatible-
   looking bytes. Ordinary execution remains available without runtime LLVM.
5. At runtime, bound layout first, resolve cross-stencil branches/data/helper
   addresses, check overflow and relocation instruction masks, then publish once.
   Handle ARM64 paired/page-relative relocations and branch reach through a small
   supported allowlist; bounded veneers/literal indirection or safe rejection
   handle out-of-range targets. Never silently truncate a displacement.

Use a Rust-declared `extern "C"` entry/exit trampoline and a separate documented
internal continuation ABI. `extern "C"` and `repr(C)` are ABI/layout contracts,
not C source or a Clang dependency; retain them where required for safety.
All connected templates must agree on registers, stack layout and transfer rules.
Generate explicit branches/jumps for internal continuations and verify stack
balance, preserved registers and bounded backedges on each supported target.
Never invoke a custom-convention interior through a Rust `extern "C"` pointer.

Rust `--emit=obj` is suitable for whole-function leaf artifacts with an explicit
FFI contract and validated dependencies. It does not make arbitrary Rust functions
concatenable. Never cut off compiler prologues/epilogues by byte heuristics or
assume Rust's default ABI is stable. Helpers use explicit C-compatible layouts,
status returns and canonical roots; JS throws are values/completions, not native
unwinding. Rust panics must not unwind through generated frames: contain them at
an appropriate Rust boundary or use an explicit fatal policy, without converting
a post-effect panic into a retryable JS miss.

First prove extraction, relocation, ABI transfer and actual execution with a
constant, a two-stencil arithmetic chain and a bounded native backedge. Then
migrate existing templates in vertical slices; preserve passing native coverage
throughout. Full infra requires this real generation path, not only a future
generator interface around manually maintained byte arrays. Replace any newly
introduced stencil-specific C templates, Clang commands/dependencies, flags and
documentation with this pipeline; preserve unrelated platform C ABI integration.

Implementation acceptance for the Rust extractor: parse object bytes with a
Rust object-format reader, not `nm`/`objdump` text or a flattened `.text` dump.
Use section-relative symbol identity and validated explicit bounds; where native
symbol sizes are absent, use declared end labels or isolated validated sections,
never the next unrelated symbol. Resolve local relocations too: no undefined
symbols does not mean position-independent or relocation-free code. Declared
external patch holes are legal, while undeclared references are rejected.
Select recipes through typed catalog entries, not a second operation-name-to-
expression match table. Each artifact owns one byte slice referenced by derived
views. Generation tests require nonempty expected coverage and actual runtime
selection of the generated artifact, not silent fallback to older byte tables.
Physical effect verification must reject unrecognized instructions or use a
validated decoder for the admitted subset; byte-pattern presence and partial
clobber scans alone are not safety proofs. Labels/relocations distinguish internal
continuations from helper calls and data from executable instructions.

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

### Performance-sensitive patterns and LLVM contracts

LLVM optimizes ordinary Rust template bodies at build time, not across fragments
patched together at runtime. Assembly interiors are opaque to those optimizations.
Use a bounded set of fused physical recipes when cross-operation optimization
is justified; retain general fragment composition and the same semantic verifier.
Do not promise full LLVM optimization from runtime concatenation.

Test reusable patterns within the supported core: loop-carried arithmetic and
ordered reductions; dense indexed updates with aliasing; repeated own/prototype
property access across mutation; alternating numeric/tagged facts; nullish and
truthiness control flow; local/captured live state across calls and exceptions.
Each test pairs stable fast facts with a fact-breaking case and verifies exact
ordinary behavior. Unsupported call/capture/exotic interiors use existing B
boundaries, not newly mandated native families.

Only export compiler assumptions established by Proven facts or dominating guards
whose validity survives intervening effects. Never manufacture `noalias` through
overlapping Rust mutable references, or keep references/raw buffer addresses alive
across invalidating helper calls. Raw pointers still require valid provenance,
alignment, lifetime and bounds; guard before creating a reference or accessing it.
Use checked arithmetic for Number specializations and explicit wrapping/masked
operations for JS bitwise behavior. Rust float-to-integer casts alone are not JS
ToInt32. No unchecked assumptions, invalid enum/bool values or speculative loads
may turn JS guard failure into Rust/LLVM undefined behavior.

Preserve signed zero, NaN behavior and evaluation order: no fast-math, reassociated
floating reductions or implicit fused multiply-add that changes JS results.
SIMD and CPU-specific variants are optional and require proven dependence/alias
safety, correct tails/bounds and target-feature admission. Generic artifacts stay
on the declared baseline; unsupported feature variants reject before entry.
Record effective compiler flags/features and inspect representative optimized IR
and object code for assumptions, spills, calls and transfers. Inspection supports
tests; it is not proof of JS semantics or performance. Do not add a runtime LLVM
optimizer or an unlimited variant cross-product to satisfy these requirements.

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

### Unit tests for infrastructure, core bodies and efficiency patterns

Unit coverage is mandatory, alongside actual native and normal-driver integration
tests. Build-script helpers need an explicitly runnable Rust test target; merely
placing `#[test]` in build-script sources does not establish executed coverage.
Generate mechanical catalog/ABI/target case matrices with Rust macros, but derive
semantic expectations independently from canonical ordinary execution and explicit
edge cases. Do not compare two outputs generated from the same mistaken metadata.

- Infrastructure: object bounds/symbol identity, local and external relocations,
  exact signed limits/alignment/instruction masks, transactional patch failure,
  wrong ABI, publication failure, stale entry rejection, reentrant retirement,
  roots, exact exits, interruption and no replay after committed effects.
- Core bodies: each required N/G family has executable input/output and guard-miss
  tests, including numeric extremes, aliases/live-outs, binding restrictions,
  property/prototype mutations and dense-array bounds/holes. Each B family has
  exact tested ordinary-boundary behavior. Unsupported hosts report skips, not
  successful native execution; supported native runs require nonzero coverage.
- Efficiency: after admission/warmup, stable hits perform no rendering/repatching
  or executable allocation; leaf dispatch introduces no unnecessary allocation;
  composed interiors have no per-operation/per-iteration Rust bridge; native loop
  initialization runs once. Test these through scoped actual-event witnesses.
- Boundedness: equivalent valid specialization keys reuse code; distinct ABI,
  layout or invalidation facts cannot alias. Churn/retry/caches/code stay within
  policy caps; retired-live allocations remain charged until leases release;
  eviction/disposal restore the documented baseline. Cold/Unknown paths avoid
  speculative rendering and unnecessary per-site executable ownership.

Use deterministic event counts, bounded byte accounting and state transitions,
not wall-clock/RSS/Bun thresholds in unit tests. Derive bounds from declared
policy and workloads; do not freeze incidental instruction sequences or forbid
legitimate JS allocation/safepoint costs. Isolate instrumentation from parallel
tests and keep it out of production hot paths. Actual timing/RSS and comparative
efficiency remain micros evidence after the full infrastructure gate passes.

## 9. Completion gate and handoff

Deliver in this finite order: (1) verified typed publication/lease and unified
installation state; (2) generated catalog and reusable region composition;
(3) every N/G core family in section 3 with actual emitted bodies, plus every B boundary;
(4) complete normal-driver integration and cross-layer adversarial verification;
(5) micros. Wire and test each vertical slice during steps 1-3, but do not start
benchmark-led tuning early. Infrastructure with catalog-only placeholder stencils
does not pass, and native bodies without normal-runtime reachability do not pass.

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
- [CPython JIT internals](https://github.com/python/cpython/blob/main/InternalDocs/jit.md): compile templates to objects and extract stencil artifacts.
- [Rust assembly](https://doc.rust-lang.org/reference/inline-assembly.html): labeled target assembly and explicit function-boundary contracts.
- [rustc code generation](https://doc.rust-lang.org/rustc/codegen-options/index.html) and [Rust function ABIs](https://doc.rust-lang.org/reference/items/functions.html): compiler controls and explicit ABI boundaries.
- [AsmJit `CodeHolder`](https://asmjit.com/doc/classasmjit_1_1CodeHolder.html): target-bound sections, labels, relocations, flattening and relocation as one code-data container.
- [AsmJit `JitRuntime`](https://asmjit.com/doc/classasmjit_1_1JitRuntime.html): a distinct allocation/publication/release edge, used here only as an organizational comparison.
- [ECMAScript ordinary/exotic object behavior](https://tc39.es/ecma262/2024/multipage/ordinary-and-exotic-objects-behaviours.html): observable get/set/receiver semantics.

These inform the design; they establish neither Quench performance nor a required
stencil count. Repository rules and observable JS behavior remain authoritative.
