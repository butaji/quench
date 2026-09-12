# Rust-generated copy-and-patch JIT

## Target and existing foundation

Build a general baseline JIT for ordinary JavaScript on Apple M4/macOS from shared
Rust operation declarations. Reuse current catalogs, rustc artifact extraction,
physical selection, region layout, installation and arena ownership. Existing
whole-function recipes are migration inputs, not proof of general compilation.

A declared opcode, selected artifact, successful guard, Rust helper invocation and
actual generated-code entry are different observations. Tests must identify which
occurred. Current policy and supported artifacts must be inspected before claiming
production native coverage.

## Generation and composition

Canonical declarations and the shared operator catalog supply operand roles,
effects, result representations,
specialization predicates, helper boundaries and physical ABI facts. Rust macros
derive mechanical views. rustc compiles templates offline; runtime code selects,
copies, patches and publishes artifacts. LLVM's offline optimization does not
optimize across independently copied fragments.
The build emits an exhaustive `Op::LOWERING_MATRIX` from those declarations;
each canonical variant is represented once, its physical row borrows the
catalog's `OperationSpec` facts, and typed cold aliases are checked against the
physical opcode catalog. The generated `Op::physical_opcode` view is derived
from the same mapping for primary physical consumers; a typed-cold alias may
differ (for example `Call`/`CallSlow`). `Slow` is the shared fallback gateway,
not an additional semantic operation.
Dedicated and legacy flagged binary spellings resolve their physical leaf
through the generated opcode and region tables; unsupported operators fail
closed when no artifact exists rather than entering a second runtime map.

Represent recipe families as data: transfers, scalar expressions, predicates,
memory access, calls, loops and allocation. Patch operands, constants, offsets and
continuations. Use shared CFG/use-def/effect analysis for admission; migrate fixed
instruction windows and function-entry recognizer chains to that analysis.
Source-shaped `Op::Loop` flattening remains a frontend lowering step, not an
admission predicate; once encoded, every baseline region is selected from the
shared CFG facts.
Retain specialized terminal covers only with generalization and profitability
evidence.

Each artifact owns bytes/data, entry offsets, architecture/features, input/output
roles, clobbers, helper effects, relocations, successor roles and identity.
External ABI equality alone cannot prove concatenation safe. Derive Rust context
layouts and template offsets from the same declarations.

The target baseline compiler covers the canonical instruction stream: native
fragments for supported operations and explicit shared-helper transitions for
others. The existing pure-numeric family is a verified foundation. The
completed baseline CFG milestone includes the minimal verified boolean
constant-arm branch witness and a
CFG-derived separated-arm constant branch admission, plus a structural one-op
`ForI` Bridge gateway for serialized residuals when its
artifact is executable. The bridge is a canonical-handler gateway, not general
loop emission; the baseline CFG authority owns CFG-derived body/update/backedge
lowering.
It also includes guarded branch-only Boolean transfer (limited to a
straight-line proven Boolean definition and returning the canonical successor), a register-valued
`JumpIfFalse -> Return × 2` witness and a guarded `Move/Jump`-to-join witness,
while continuing toward general CFG control flow; the completed cross-tier authority adds effectful/helper transitions. Helpers are
legitimate semantic boundaries; a generic per-op Rust dispatcher is not evidence
that native operation coverage is complete.
Admitted Bridge regions (including the four-operation `binary_glue` shape and
the conditional `binary_branch_glue` witness) are generated interior covers:
canonical loads, effects and return remain authoritative while existing scalar
arithmetic leaves execute guarded numeric binary operations. The executor
follows verified forward joins and resident backedges from the shared CFG; a
verified Boolean `JumpIfFalse` uses the physical word-branch composer, and a
physical miss falls back for the current operation after the already-committed
prefix. Control-only Bridge regions retain this CFG executor on a guard miss,
so canonical joins and exits use the same verified transition path. Verified
unconditional `Jump` edges use the physical word-control image with a constant
Boolean transfer and retain canonical fallback on publication failure.
Supported immutable numeric, Boolean, Null and Undefined `LoadConst` operations
use their generated tagged-word leaves on that same CFG path. Pure `Move` operations use
the generated tagged-word copy, while ownership is committed by the canonical
destination write. The flags=1 `Move` spelling is the validated proven-local
form: its third operand names the environment target, and the runtime guard
must prove both slots before the environment commit. Proven `LoadLocal` words
use the same leaf only when the
environment exposes a stable tagged-word pointer; the `store_glue` witness
uses the same word copy for a proven direct `StoreLocal` slot only when the
environment can commit the tagged word without changing binding semantics.
The `inc_glue` witness consumes distinct generated `IncI` `+1` and `-1`
artifacts, preserving the direction flag through selection and patching.
Checked local pairs use the same leaves only after their initialized/deleted
binding guards prove the direct slot transfer.
`LoadParameter` uses the same stable environment word load through the
`parameter_glue` witness.
The existing `loop_body` witness consumes checked-load, direct-store, and move
leaves around its canonical `UpdateLocal`; its environment commit remains an
explicit semantic boundary.
The `counted_glue` witness follows a verified internal backedge with physical
comparison, branch, jump, and `IncI` leaves, polling before each resident
re-entry while retaining exact canonical retirement.
`counted_decrement_glue` proves the same resident path for a `GreaterThan`
condition and the distinct generated `IncI -1` artifact.
`counted_continue_glue` proves nested skip/break branches, a resident continue
backedge and forward exits in one shared CFG plan.
`nested_branch_glue` proves two nested conditional blocks with three arm
entries and one shared join, using the same CFG edge validation and exact
canonical arm retirement.
For regions without a named recipe, admission may retain the verified
canonical opcode slice as a dynamic Bridge plan, including regions with
multiple terminal `Return`/`Throw` exits. The generated dispatch artifact and
generated leaves consume supported operations; unsupported operations take the canonical
handler, and any CFG or physical-contract miss rejects before effects. This is
generic safe traversal, not a claim of complete native emission for every
opcode or of production OSR-aware residency. Leaf plans are initialized once
per region plan, then reused across resident re-entry. The focused OSR witness
enters a validated Bridge region under the diagnostic opt-in; the baseline CFG
authority owns the CFG entry/live facts and baseline retirement, while the completed cross-tier authority owns the actual
cross-tier frame handoff and helper-boundary protocol. Production Apple-arm
residency remains unqualified until it passes the same broad contract.
Dynamic Bridge admission is value/control-only: the candidate prefix stops at
the first operation carrying a heap read, heap write, allocation or observable
effect. That helper/allocation boundary remains canonical whenever the shared
activation and root protocol cannot prove a safe transition; a proven prefix is still eligible
for generated execution.
Admission derives and caches each pure value/control prefix endpoint from the
complete static instruction facts, then derives the candidate from that view
(generic operator and unary flags, move-local spelling, global-slot
initialization and range-owned constant operands); environment-dependent slot
proofs remain execution guards. Thus unsupported flagged `Binary` operators,
`Remainder`, `Exponentiate`, `Instanceof`, global-slot `InitLocal`,
unsupported string/BigInt
`LoadConst` operands, and non-number `AddConst` operands do not publish a bridge
merely because their opcode name appears in the canonical suffix. Proven
move-local copies use the tagged-word leaf with an environment-owned target
commit; cell-backed, immutable, deleted, uninitialized or malformed moves
remain canonical.
The broad Generic Bridge eligibility bit is generated from the canonical
opcode declaration into the shared `OperationSpec` and checked against its
effect/control facts. The runtime keeps only payload-sensitive checks beside
it; this is the single coverage authority for bridge admission, not a parallel
opcode allow-list.
`Remainder` and `Exponentiate` stay canonical even for numeric operands until
their artifacts are self-contained: rustc's floating-point remainder currently
emits an external AArch64 PLT relocation, which the stencil extractor rejects.
A helper-backed implementation must first declare its helper/root ABI and exact
fallback boundary; a scalar opcode name alone is not an executable artifact.
Terminal `Return` operations in dynamic Bridge regions may consume the
generated tagged-word return artifact on supported native hosts (AArch64 uses
the x0-in/x0-out leaf; x86-64 uses the explicit rdi-to-rax move); Rust
materializes the owned completion value, and any host without a validated
artifact uses the canonical return handler.
Terminal `Throw` operations use the same generated dispatch boundary to
materialize `Completion::Throw` after the canonical register read; invalid
register state retains the canonical error path.
`AddConst` is a supported scalar leaf in this path only when its constant-pool
operand is numeric and right-sided; constant-left and non-number inputs use
the complete canonical arithmetic handler.
`UpdateLocal` is also admitted through this generic path when its direct local
slot is proven numeric and writable; the generated `IncI` leaf computes the
value, while the environment remains the canonical ownership commit. Immutable,
cell-backed, uninitialized and non-number slots use the canonical handler.
The canonical `NumericAdd`/`NumericSubtract` opcodes, plus legacy generic
`Binary` instructions carrying those operator flags, use the same generated
increment/decrement families. Their encoded RHS is intentionally ignored by
the semantic contract; ordinary binary add/subtract remains a separate family.
An arbitrary dynamic region may therefore retain a numeric backward `Jump`:
the shared CFG validates the edge, truthiness and update leaves re-enter the
same region, and the interrupt poll returns to the canonical loop boundary.
Bitwise and shift opcodes use their generated integer leaves only after the
numeric inputs pass the shared conversion guard; all other values remain on
the canonical conversion path.
Strict equality/inequality may use the generated word-pair leaf for proven
identity values; numeric inputs use the scalar comparison leaf and coercive
values always return to canonical semantics.
The `unary_glue` witness consumes generated numeric `+`, `-` and bitwise-not
leaves inside a CFG bridge; unsupported unary operators and non-number inputs
remain canonical.
The same bridge can materialize `Unary(Void)` and `Unary(Delete)` through
generated immutable Boolean/undefined-word leaves; their sources are still
evaluated by the canonical producer, and no coercion is introduced.
`InitializeLocal` now consumes the active environment's TDZ transition inside
the verified bridge; it does not copy value bits or bypass the environment
authority.
`InitLocal` can likewise consume the value-bearing declaration transition when
the active environment accepts the tagged-word copy; its retain/release and
TDZ publication remain environment-owned, and unavailable slots or sources
use the canonical handler. Global slot zero retains the canonical handler
because initialization also publishes the global-object representative.
The metadata-only `MarkUninitialized` and `MarkImmutable` transitions use the
same environment authority, preserving shared-TDZ flags and immutability
guards while avoiding an extra baseline dispatch.
`CheckInitialized` can likewise retire as a native metadata read only when the
active environment proves the slot initialized; TDZ failures stay on the
canonical handler for its exact `ReferenceError`.
Control-only bridges may also consume the generated truthiness leaf for
numeric and tagged-word conditions before `JumpIfFalse` or logical-not; heap
coercions that require semantic helpers remain canonical.
The same unary bridge consumes the generated nullish word predicate for
`IsNullish`, preserving the exact Null/Undefined distinction.
The `update_return` and `loop_body` witnesses reuse the generated `IncI` leaf
only for proven direct numeric local slots; immutable, cell-backed,
uninitialized or non-number slots remain canonical.
The [task ownership map](../tasks/README.md#the-ownership-map-is-deliberately-small)
is the sole roadmap authority for these boundaries: the baseline CFG authority owns CFG shape,
liveness and baseline retirement; the value/backing authority owns value/backing identity; the
completed cross-tier authority is the consumer allowed to combine them at a tier or helper transition.
Later tasks derive views from those authorities rather than introducing another
frame, value or control representation.
Each branch witness consumes verified source CFG facts before composing. A
projected physical arm layout is permitted only after independently validating
both source arms, their exact continuations and the joined live-out. Any
mismatch rejects to canonical execution.
The generated ABI contract marks which rows have a complete
`NativeRegionContext` consumer. Today that set is Bridge, ArrayKernel,
ArrayNumericLoop and AffineI32Loop. Generic admission reads that generated
fact; typed-only loop rows are selected by their owning typed plans. If that
plan or artifact is unavailable, admission rejects before effects and the
canonical path remains authoritative.

## Control, representation and exits

Build symbolic labels and checked relocations from the canonical CFG. Use its
verified sorted basic-block spans, preserve join values, complete live-outs
across external exits (cached on the immutable region plan for native exit
materialization), distinguish backward control exits from internal
backedges, and derive conservative induction candidates and loop-carried state,
evaluation order and exact continuation PCs.
Keep intermediates in registers where profitable; synchronize live/rooted state
at calls, safepoints and exits. Register allocation, spills, broader dataflow,
inlining, SIMD and optimizing tiers are permitted when their proofs and costs
are explicit.

Native array views carry the canonical ownership fact from `ArrayData` (stable
JavaScript identity, element kind and backing generation). A structural backing
change invalidates the view; value-only numeric writes retain the generation
when their element kind is stable. Writes that widen the element kind or delete
an indexed element also advance it. Native consumers must validate the stamp at
the committed exit before
publishing a result.

A guard must dominate its use and remain valid after intervening effects. Shape
does not imply callee identity, field type or prototype immutability. Preserve
Number rounding, NaN, signed zero, BigInt rules and ToInt32 behavior; no unproven
reassociation or fast-math assumptions.

The generic region fallback follows verified forward branches through joins and
carries the admitted `RegionControlPlan`, checking each transition against its
edge set before re-entry rather than accepting an in-range target by address
alone. A generic Bridge may retain a valid edge to another canonical PC outside
its contiguous prefix, including a helper boundary or enclosing loop. The
exclusive code-end PC is also a legal external normal exit; a target beyond the
code range is malformed and rejects to canonical execution.
Branch-only Boolean artifacts must return the tagged Boolean they tested; any
other physical result is a miss and re-enters the canonical `JumpIfFalse`
handler without guessing a branch arm.
CFG-admitted internal backedges may remain in its canonical resident loop, with
an interrupt poll before each re-entry; non-resident or pending-safepoint
backedges return to the ordinary driver so interruption and OSR stay at a
canonical loop boundary. That canonical residency is an interruptible fallback
path, not general generated native loop-body execution. The bridge reports exact
canonical-handler retirement, including resident re-entries, for tier accounting.
Native exit outcomes
distinguish rejection before
effects, successful completion, semantic throw, and continuation after
committed effects. A post-effect miss must
never replay earlier work. Await/yield/finally and native-to-host reentry retain
the same completion and ownership contracts as ordinary execution.
Native array and affine-loop kernels poll at every backedge and admit any
semantically valid trip count; counted, typed-lane and numeric reduction kernels
likewise have no arbitrary iteration cutoff. Any bound retained by a specialized
cover must document a compile-cost, numeric-safety or physical-ABI capacity
purpose and preserve complete canonical fallback behavior. Such representation
budgets do not constrain the general CFG path or JavaScript semantics.
CFG-relative numeric-loop recipes derive their seed, bound, backedge and
continuation PCs from the admitted CFG start; other call-recurring
recipes retain their legacy whole-function metadata contract until their
call/exit transition is generalized. The local-recurrence cover (including its
loop-header OSR view) is narrow migration support for the canonical
29-operation lowering and reuses the affine artifact, resuming the loop-header
view from the live value/index locals; it is not general CFG loop-body emission.
OSR compilation may use a cheap raw backward-edge prefilter, but a live-frame
handoff requires the newly published baseline plan to expose the same valid CFG
backedge target; otherwise execution remains on the canonical dispatcher.

## Publication and lifetime

Verify relocations, ranges, alignment, helper addresses, CPU requirements and ABI
before publication. Maintain macOS executable-memory protection and instruction
cache coherence. Failed publication leaves no callable partial image.

Published image metadata is immutable authority. Cache hashes accelerate lookup;
reuse still validates identity. An active entry holds its code lease; eviction
cannot free executing bytes. Track resident, cached and retired-live allocations
without double counting. Unknown or unsupported paths fall back safely.

Compilation, cache and region budgets require documented cost or correctness
reasons and observable exhaustion behavior. Selector-local eight-value,
fixed-window and input-count limits are implementation choices to revisit, not
design ceilings; derived CFG blocks and edges grow with the verified region and
only allocator exhaustion rejects the plan. These policies must never narrow
unrelated baseline admission. Growth must be tested for compile latency, code
size, memory, interrupts and fallback.

## Required proof

For every migrated family, demonstrate ordinary source -> lowering -> production
admission -> selected generated artifact -> actual execution -> exact exit.
Pair positives with renamed locals/registers, varied constants, harmless prologues,
aliases and zero/one/many iterations, plus a guard-breaking case.

Cover wrong ABI, missing artifacts, relocation limits, publication failure,
stale/ABA identities, cache churn, active leases, roots, helper reentry, exceptions,
suspension and interruption. Validate generated-enabled and unavailable-artifact
behavior; actual M4 execution is required for native claims.

Stable hits reuse published code without repeated patching/allocation. Compilation,
dispatch, spills, boxing, helpers and exits have separate measurements. Functional
infrastructure can close without a speed claim. Performance acceptance follows
[the protocol](performance-lanes.md), at each meaningful milestone.

[Task queue](../tasks/index.json) owns scope and completion. Its
`critical_path` is the VM sequence; profiled subsystem and host cleanup lanes
are optional side work and must not block physical-JIT milestones.
[The native generation reference](native-generation.md) informs generation; [architecture](architecture.md)
owns shared execution and value contracts.
