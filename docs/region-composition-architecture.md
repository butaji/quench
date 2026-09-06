# Bounded region composition: research decision

Status: architectural recommendation, not a completed implementation or measured
speedup. Current-source benchmark refresh is pending a coherent buildable snapshot.
Read with [activation](activation-architecture.md) and
[heap identity](heap-identity-architecture.md); these are shared contracts, not
three new runtime subsystems.

## Existing foundation and the actual question

`machine.rs` already has `NativeRegionPlan`, residual-window validation,
`region_admission_matches`, successor traversal and live-out analysis.
`stencil_select.rs` has generated region contracts and bounded rendered-region
storage. Do not describe CFG validation, liveness or regions as wholly absent.
The architectural question is whether these mechanisms compose useful regions
with stable representations and bounded cost, beyond matching predefined windows.
Audit their consumers and emitted code before declaring any capability complete.

Historical NavierStokes traces show many packed accesses alongside local reads
and copies; this motivates residency investigation, but does not prove those
events dominate CPU time. Require fresh profiles, disassembly and controlled
scalar-versus-composed execution on the same source before claiming benefit.

## Alternatives

| Choice | Benefit | Limitation |
| --- | --- | --- |
| Scalar stencils only | Small predictable materializer | Retains operation boundaries and transfer costs |
| Enumerated whole-loop templates only | LLVM sees the whole template offline | Coverage and variant growth; arbitrary loops do not become enumerable |
| Bounded composition of residual operations | Reuses facts, contracts and offline templates | Requires explicit joins, effects, exits and transfer accounting |
| Runtime optimizing compiler | Broad optimization and allocation freedom | Violates the selected runtime compilation constraint |

Recommend bounded composition plus measured fused templates. No second semantic
IR, unrestricted optimization search or global register allocator. This is a
trade-off, not evidence that arbitrary optimizing-JIT performance is attainable.

## Data contract

- Immutable code metadata supplies successors, operand roles, definitions/uses
  and effects. Derive those from existing Rust operation declarations.
- Site observations remain observations. Region admission consumes `Proven`,
  `Guarded` or `Unknown` facts with their validity domains; it does not create an
  independent profiler/type system.
- A disposable plan references original code/PCs, selected templates, patch
  bindings, fixed ABI roles, transfers and live exit state. It owns no alternate
  JavaScript semantics and no durable duplicate program graph.
- At joins retain only facts valid on every incoming path. At loop headers,
  require explicit backedge-compatible representations; otherwise split or use
  ordinary execution. Never infer invariance from a stable training sample.
- Shape, backing address, length, element kind and constant field value have
  distinct validity. Calls, coercions, accessors and allocation may invalidate
  different facts. Unknown effects conservatively end the affected specialization.

## Bounded algorithms and offline work

Freeze reusable structure during code finalization rather than rediscovering it
on hot entry. Bound candidate operations, edges, live values, pattern probes,
inlining expansion and emitted bytes. Exhaustion is a normal fallback reason.
Use deterministic traversal with explicit conservative handling of unsupported
joins/loops; any liveness fixed point needs a documented work bound and fallback.

Match candidates by estimated saved dispatch/guards/boxing minus transfers,
spills, exits and code bytes. Longest match is only a tie-breaker, not a cost model.
Start with fixed placement contracts; select precompiled transfer/spill forms or
split. Do not quietly turn placement selection into unrestricted runtime register
allocation. Record why candidate regions were rejected and the cost of planning.

Rust macros generate contracts, matcher metadata and test schemas. Rust source
is compiled with rustc/LLVM offline, then extracted with validated relocations.
LLVM optimizes within an offline template; copying adjacent templates does not
enable cross-template optimization. Ordinary Rust calls do not promise a custom
register-preserving or guaranteed tail-call ABI. Require machine-code evidence
for every supported target contract, not source-level inference.

## Acceptance and sequencing

### Cold planning is distinct from retained native storage

Current BaselinePlan retains sparse admissions plus a per-PC span index, not one
per-family dense retained table. Its constructor nevertheless builds multiple
temporary Vec<Option<Rc<...>>> arrays over all entries and computes liveness
before composing the retained result. Constructors can reject native policy, but
that does not itself remove the surrounding collection passes. Measure transient
allocation and cold plan latency separately from final admission bytes.

SharedStencilSlab::new creates an empty slab list; the 4096 capacity argument is
not evidence of immediate executable-page allocation. Avoid that false diagnosis.
Likewise liveness already has a round cap; it is not an unbounded fixpoint. It
uses per-PC BTreeSets and repeated successor/set work. Audit convergence handling
and work/byte cost, not merely whether a loop has a numerical bound.

Compare one generated dispatch over operations that appends eligible admissions
directly against the current multi-pass constructor. Preserve alternatives and
deterministic precedence. Derive reusable CFG/use-def facts at code finalization;
defer optional native-only analysis until a policy/admission requires it. Choose
compact dense or sparse liveness representation from actual register density;
do not replace every set with a maximum-register bitset by rule. Budget exhaustion
must disable the affected specialization conservatively, not imply dead values.

The initial RegExp sample includes BaselinePlan::compile but cannot establish
recurring compilation or its steady-state share. Validate function-size scaling,
cold-only functions, repeated entry/reuse and native-off controls. Report transient
peak bytes, retained bytes and compile/reuse counts along with end-to-end timing.

075 retains its infrastructure/correctness gate. This recommendation does not
silently add an arbitrary pattern-count quota to that gate. 073 experiments must
compare scalar and composed modes with identical semantics and source identity.
Measure planning latency, template/static bytes, resident native bytes, transfers,
guards, exits, fallback rate, throughput and fixed-work RSS; keep counter counts
separate from CPU shares. Include held-out shapes/sizes, mutation, callbacks,
overflow, exceptions, suspension and budget exhaustion. Unit tests cover contract
composition and exit reconstruction; differential tests establish JS behavior.

## Research basis

[Maglev](https://v8.dev/blog/maglev) uses a bytecode prepass, liveness, fact tracking,
deoptimization maps and representation selection. It is evidence that these
contracts matter, not a proposal to import its SSA compiler into Quench.
[Copy-and-Patch](https://arxiv.org/abs/2011.13127) motivates offline templates and
runtime patching. [Deegen](https://arxiv.org/abs/2411.11469) motivates deriving
execution machinery from shared semantic descriptions. Neither establishes the
optimal stencil count or guarantees this design will beat V8 or Bun.
