# Quench VM goal

## Outcome

Build a semantically complete JavaScript VM whose mechanical execution code is
generated from declarative Rust macro facts. The design follows the useful
Deegen principle—one operation description produces the VM views—without
copying its C++ toolchain or enabling a JIT at this stage.

The current execution ladder is deliberately interpreter-first:

```text
OXC / Wasm frontend
  -> fact-checked lowering
  -> canonical operation stream
  -> generated compact decoder and interpreter
       -> complete ordinary semantics
       -> Proven / Guarded physical variants
       -> bounded quickening and inline caches
  -> reusable generic fast paths
       -> complete ordinary fallback
```

There is no JIT, executable-memory allocator, OSR, deoptimization protocol,
machine-code cache, or benchmark-specific execution path in this goal, with
one bounded, explicitly gated exception: a copy-and-patch **region**-stencil
tier (`tasks/021.md`-`tasks/025.md`, theme `copy_patch_jit`, plus prerequisite
`tasks/026.md`) built as a precomputed code cache with parameter binding, not
a compiler — the Deegen paper's actual architecture (arXiv:2411.11469) is a
continuation-passing interpreter plus this same Copy-and-Patch technique for
its baseline JIT tier, and this exception adopts the technique without
adopting Deegen's optimizing-JIT, OSR, or hotness-tier-up machinery. Its data
model is canonical and layered the same way the rest of this goal is: a
`RegionKey` (`hash(region_id, fact_vector)`) selects one `Stencil` (`bytes`
plus a closed set of typed `Hole`s — `Imm32`/`Disp32`/`Rel32`/`Ptr64`, no
generic relocation engine) from a build-time dispatch table; `PatchValues` is
a read-only view into the existing `QuickeningSite` shape/callee/slow-path
state, never a second copy of that fact; `BoxingFact` is a declared
description of the existing `JsValue` tagged layout (task 017) so type-check
strength reduction has one source of truth, mirroring the paper's boxing-
scheme description APIs without renegotiating the boxing scheme itself.
Stencils are fused **regions** (a loop body, a property-access chain)
admitted by existing Proven/Guarded facts, not one-stencil-per-opcode as the
paper does — this is quench's own extension beyond the paper, so a fused
region is only admitted after a build-time proof that its interior has no
externally-reachable entry point besides its declared one, keeping hot-cold
splitting and jump-to-fallthrough sound across the fused boundary. Type-check
elimination/strength-reduction runs as one named, reusable build-time
algorithm over a region's semantic function and a fact predicate (mirroring
the paper's algorithm 𝒜, §5.1), not per-operation ad hoc logic. Runtime work
is exactly `select -> alloc (bump-pointer arena) -> memcpy -> patch ->
execute`; if instruction selection, register allocation, or CFG analysis
ever shows up at runtime, that is a real JIT and out of scope. This exception
depends on task 026 first moving hot dispatch from a driver-owned loop to
callee-directed tail transitions — the paper's fast-path fallthrough
elimination (§7.1) only has meaning once "what runs next" is a callee-
supplied address, not a value a loop interprets. A site prefers **patching
data over patching code**: its named lifecycle (`Cold -> Rendered ->
Installed -> Repatch -> Retired`) rewrites only `PatchValues` when the
installed stencil's holes still cover the new fact, and only re-renders/
copies code when they do not, using the same bounded degrade-tier limit as
interpreter-side quickening (014) before retiring to the ordinary path
permanently. The `Repatch` transition is the effectful half of the same
named idempotent-probe/effectful-apply interface `QuickeningSite::observe`
already implements as its pure half (mirroring the paper's λi/λe IC split,
§5.2) — one shared interface, not independently reinvented per site.
Rendered regions are memoized by `RegionKey` so identical `(region, fact)`
combinations are never re-rendered; this memoization is eager per admitted
fact combination, never hotness-triggered — a future profiling-and-threshold
tier-up (the paper's §3) is explicitly out of scope and would need its own
task, since it reintroduces the profiling/threshold machinery this
exception's "no OSR" boundary excludes. This is not a general-purpose JIT:
there is no tracing, no speculative optimization beyond the existing
`Proven`/`Guarded` facts, no OSR, and no deoptimization protocol — every
stencil path keeps the complete ordinary interpreter as its fallback on any
Unknown fact, hole-table miss, or patch failure, and the exception is gated
behind correctness and dispatch groundwork (tasks 011, 016, 019, 026)
landing first. `tools/check-vm-architecture.cjs` enforces that the
`copy_patch_jit` theme exists in `tasks/index.json` only together with this
paragraph, and that every `copy_patch_jit` task depends on one of those gates
or another `copy_patch_jit` task, so the exception cannot silently drift from
documented, bounded scope.

## Architectural rules

1. Represent each semantic fact once.
2. OXC owns JavaScript syntax; Quench does not build a second syntax tree.
3. Keep static structure as data or eliminate it before runtime.
4. Let VM code represent only dynamic uncertainty.
5. Semantic abstractions do not imply runtime allocations.
6. Share semantic mechanisms while specializing physical execution.
7. Generate mechanical consequences from one declaration.
8. Give no subsystem its own semantic universe unless its rules require one.
9. Treat types and profiles as facts, not another runtime or optimizer.
10. Classify facts as `Proven`, `Guarded`, or `Unknown`.
11. Complete slow semantics and cheap `Unknown` behavior precede fast paths.
12. Never optimize through observable JavaScript behavior.
13. Keep heap references compact and count generated code, static data, caches,
    and native code in the complexity budget.
14. Keep optional physical execution bounded, disposable, and semantically
    subordinate to the ordinary interpreter.
15. If something can disappear before runtime, justify why it exists.

## One declarative macro

`vm_op!` is the only operation declaration surface. It is a declarative,
analyzable Rust macro—not an attribute system and not an arbitrary Rust-body
expander. The declaration records operation identity, encoded width, effects,
result shape, control exit, guard requirements, and the named semantic
fallback. Its syntax is intentionally compact while the fact schema grows;
variants are added as fields to this same macro rather than as a second
declaration system.

The current spelling is `Name = opcode / operand_width => [effects] / fallback /
result / control / [guards] / handler`; an optional final operator name supplies
generated arithmetic mapping. Future fact fields extend this record without
changing the declarative-macro boundary.

```rust
vm_op! {
    Add = 3 / 3 => [MayThrow] / add / Value / Next / [] / run_arithmetic / Add,
    GetProperty = 12 / 3 => [ReadHeap, MayThrow, Observable] / get_property / Value / Next / [Shape] / run_compact_get_property,
}
```

For example, the `Add` row currently derives a single catalog record and a
fixed-width construction path:

```rust
let add = Opcode::Add
    .builder()
    .operands(dst, lhs, rhs)
    .build()?;
assert_eq!(add.opcode, Opcode::Add);
assert!(Opcode::Add.has_effect(OperationEffect::MayThrow));
```

The builder does not implement addition. It only carries the declared shape;
the named `add` fallback remains the semantic owner for ordinary execution.

The semantic functions and gateways remain small, explicit Rust functions.
The macro emits only mechanical consequences; it does not hide observable
behavior in token tricks or source inspection.

## Generated views

From each `vm_op!` declaration, generate:

- stable opcode IDs and names;
- compact instruction constructors and operand decoders;
- operand-width and register/constant validation;
- operation metadata (`effects`, result shape, control exits, guards, and fallback);
- uniform opcode-to-handler dispatch entries;
- ordinary interpreter dispatch;
- Proven and Guarded physical variants;
- bounded quickening and inline-cache state transitions;
- invalidation and miss edges back to the complete fallback;
- generated table and differential tests.

The `vm_op!` macro in `crates/quench-runtime/src/ir.rs` is the migration seam:
it derives opcode metadata, the catalog-backed compact instruction builder,
operand decoder, control-operand roles, handler table, and quickening
eligibility from one declaration. Semantic handlers remain ordinary Rust
functions selected by the generated table; no second operation table or
attribute/proc-macro layer is introduced. Residual loop control, the exact
shared JS/Wasm lowering overlap, and differential evidence are covered by the
acceptance checks in `tasks/index.json`; future operations follow the same
single-declaration boundary.

## Fact and fallback model

Facts are data:

```rust
Fact::Proven(Number)
Fact::Guarded {
    value: Number,
    guard: Shape(shape_id),
}
Fact::Unknown
```

`Proven` may select a specialized implementation. `Guarded` emits a cheap
runtime check and a miss transition. `Unknown` immediately uses complete
ordinary semantics. A miss never restarts already-observable work; it resumes
from the explicit residual state or returns to the ordinary operation at the
same semantic point.

Effects include allocation, heap reads/writes, coercion, calls, exceptions,
proxies, host I/O, scheduling, and suspension. Pure fragments may be shared;
effects stay at named gateways.

## Runtime boundaries

- `quench-runtime` owns JavaScript semantics, operation facts, generated VM
  views, allocation, and gateways.
- `quench-node` owns only the Node-compatible host/API boundary.
- OXC owns JavaScript parsing and reduction inputs.
- Wasm tooling owns decoding and validation; shared operations use the same
  fact schema where semantics overlap.
- No second syntax tree, semantic runtime, collector, object model, or guest
  execution universe is introduced.

## Production and benchmark integrity

Production code must not inspect fixture names, source text, scores, checksums,
suite markers, or engine identity. Workload-specific kernels and their fact
recognizers are not VM semantics and do not belong in the runtime. Reusable
kernels are allowed when selected only by semantic facts, bounded, and paired
with complete ordinary fallback behavior.

Every optimization must be reusable outside its originating workload, guarded
by facts, and fall back to complete ordinary behavior. Benchmarks measure the
same artifact users run; they do not select code paths.

The same rule applies below the operation level, to guards and caches: a
guard, quickening site, or degrade tier may admit on runtime facts alone —
value tags, shape identity, object layout, callee identity — never on which
fixture, corpus, or file produced them. "General-purpose kernel" means the
kernel's admission rule and bound generalize to any JavaScript program with
the same runtime shape; a kernel whose bound, threshold, or admission check
was sized to make one known benchmark's specific object shape, call count, or
loop trip count fast is a benchmark-specific bypass regardless of how it is
phrased, and is forbidden by rule 12 and this section. `tools/check-vm-architecture.cjs`
enforces the source-inspection half of this rule mechanically; the
shape-tuning half is a review obligation on every guard, cache, and
quickening change (see tasks/index.json:013-018).

## Current implementation baseline

The repository already contains:

- OXC reduction and a handwritten `Op` enum;
- compact instruction lowering and code arenas;
- a Rust VM dispatcher with ordinary fallbacks;
- `Proven`, `Guarded`, and `Unknown` fact states;
- shape, tag, call, and property cache mechanisms;
- one canonical shape/property identity vocabulary shared by caches and the
  dynamic adapter;
- separate JavaScript and Wasm frontend representations.

The operation facts, compile-time declaration validation, compact builder,
operand decoder, quickening eligibility, and opcode-to-handler table are now
generated from `vm_op!`. `AddConst`, `IncI`, and `AGetIInc` have catalog-selected
handlers, and guarded shape sites are attached to executable code and wired to
the plain-own-property fast path. A bounded catalog-backed quickening site is
the physical-state primitive; misses remain disposable and return to complete
ordinary semantics. The complete fallback remains a handwritten semantic body
behind that generated table. Generated `Jump`/`Branch`/`Return`/`Loop`
operand-role views now feed the compact completion loop. The reserved `ForI`
row has an explicit structured-loop fallback contract: it executes only when
cold metadata carries `Op::Loop`, and otherwise rejects malformed bytecode
deterministically. Ordinary lowering continues to emit structured loops, so no
second loop state representation is introduced.

The compact fallback selector keeps only opcode classification inline; the
unlowered `Slow` gateway is outlined as a cold, non-inlined body. The cold-path
audit and code-layout measurement are recorded in
`docs/architecture-evidence.md`.

The catalog declares `Number`, `Shape`, `DenseArray`, and `Callable` guards.
Shape and callable observations now share the same bounded quickening-site
state: `Call` sites admit weak callee identities, execute the complete call
gateway on installation or miss, and use the direct synchronous path only on a
callable-identity hit. Misses move through a bounded polymorphic/megamorphic
tier and may re-arm after stable hits; no site is permanently disabled. Named
method calls retain their property/receiver gateway; eligible function targets
feed the same callable state only after that resolution because resolving a
method can itself invoke observable JavaScript.
`execution_trace` now records generic quickening hit/miss facts and biases
bounded-cache probe ordering toward runtime-hot entries. These are
Deegen-shaped mechanisms in an already Deegen-shaped design, not new
architecture.

The dynamic adapter now uses an explicit fixed-width 16-byte tagged payload
for `JsValue`; execute registers and slot storage use the canonical one-word
`TaggedValue`, and both boundaries have representation/identity tests.
Shape interning,
property transitions, and per-shape slot lookup now use derived bounded hash
views; neutral property-add/read evidence is recorded for task 018.
Representation and lookup-cost changes must not change observable shape,
property, or enumeration semantics.

`Shape` now stores one derived content hash and a derived atom-to-slot index;
`ShapeTable` uses hash buckets and an explicit memoized property-transition
edge, so repeated interning and repeated
`Shape --add(atom,flags)--> Shape'` events avoid a full-table scan and repeated
shape reconstruction. The atom table already uses a hash map for string
interning, and equality
remains authoritative for all
observable behavior.

A code review against the Deegen paper's interpreter-tier techniques (pure
inline-cache key-check split from effectful apply; no hand-duplicated
per-operand-type opcode variants) found both already satisfied:
`QuickeningSite::observe` in `crates/quench-runtime/src/quickening.rs` is the
pure key-check half and is now documented as such; the effectful slot apply
lives only in callers such as `quickened_own_get`. No duplicated
`Add`-style reg/const variant families were found in `vm_arithmetic.rs`,
`vm_ops.rs`, or `vm_properties.rs` — arithmetic already dispatches on one
`BinaryOp` fact. A neutral macOS profile placed
`run_code_completion_step_from` on the hot stack, so task 019 now gives the
catalog-selected dispatch boundary an explicit `DispatchTransition` carrying
next-pc and completion. The driver consumes that value; this is data-shape
groundwork, not a step toward CPS, tail-call, computed-goto, or JIT dispatch.

## Delivery order

The canonical queue is [tasks/index.json](tasks/index.json). Its dependency
order is:

1. freeze a neutral baseline and inventory execution seams;
2. remove workload-specific production bypasses;
3. define the operation fact schema;
4. extend the declarative macro;
5. generate storage, decoding, and validation;
6. generate the ordinary interpreter;
7. generate guarded quickening and bounded IC state;
8. migrate reusable generic fast paths;
9. unify JavaScript and Wasm lowering where semantics overlap;
10. enforce generation and architecture invariants;
11. establish correctness and neutral performance gates;
12. consolidate evidence and documentation.

Every task in `tasks/index.json` carries exactly one `theme`: `deegen_codegen`
(one `vm_op!` declaration generating every VM view, including guarded
quickening/IC state), `quickjs_perf` (compact value representation and hashed
shape/property lookup), or `benchmark_integrity` (removing and permanently
blocking workload-specific execution paths, and neutral correctness/
performance evidence). The file's `themes` object restates the same 19 tasks
grouped by pillar; no task outside those three pillars is added to the queue.

Correctness is the first gate. Node-oracle comparisons must cover values,
descriptors, identity, ordering, errors, exit status, and host effects.
Performance is measured only on neutral workloads and reported with a clear
index, startup cost, memory, tail latency, and geomean.

The read-only architecture gate is `node tools/check-vm-architecture.cjs`; it
checks the single `vm_op!` declaration, task-queue integrity, and the absence
of workload-specific runtime features without executing guest or benchmark
code. Reproducible correctness, layout, representation, and neutral corpus
commands are indexed in [`docs/architecture-evidence.md`](docs/architecture-evidence.md).

Tasks 013-019 close the concrete gaps between this plan and the Deegen
paper's inline-caching/tiering ideas and the QuickJS reference
implementation's value and shape representation, without adding a JIT,
executable memory, or any fixture-shaped code path: call-site guard state
(013), a bounded polymorphic degrade tier (014), profile-fact-informed
quickening (015), cold-path outlining (016), a compact tagged value
representation (017), hashed shape/property lookup (018), and an explicit
catalog-handler dispatch transition (019).
