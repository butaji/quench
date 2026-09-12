# Runtime architecture

## Native execution core

`crates/quench-runtime/src/native_core` owns the physical execute word used by
registers, mutable slots, native entries and copy-and-patch operands. It is
compiled as part of `quench-runtime`; there is no sibling-tree build path and
no compatibility bridge around the hot representation. Quench's object model,
garbage collector, built-ins, modules, async machinery and Node host remain the
semantic authorities required for complete ECMAScript behavior.

This page is the compact map of the runtime. The normative details live with
the authority that owns them: canonical operations in Rust declarations, the
copy-and-patch execution contract in the [JIT specification](stencil-jit-implementation-spec.md),
task order in [`tasks/index.json`](../tasks/index.json), and measurements in the
[performance protocol](performance-lanes.md).

## Data flow

OXC parses JavaScript. `reduce` produces canonical `ops::Op` data. `ir.rs`
stores compact instructions and lowers those operations. `CodeStore` freezes
one `FunctionLayout` for each logical code range; frame width, register width
and parameter boundaries are derived from that record. `machine` owns code,
plans and tier transitions, while `vm` and the interpreter execute the same
semantic operations.

The build derives opcode metadata and physical views from the canonical
declarations and catalog. `Op::LOWERING_MATRIX` has one row per canonical
operation and borrows the catalog's `OperationSpec`; `Op::physical_opcode` is a
derived primary physical spelling. The catalog also derives cold markers and
dispatch coverage, including the intentional `Call`/`CallSlow` and
`Loop`/`ForI` aliases. `Slow` is a fallback gateway, not another semantic
operation. Adding an operation therefore changes one declaration and its
derived views, not several hand-maintained tables.

## Execution views

There is one semantic stream and several derived execution views:

| View | Owns | Rule |
| --- | --- | --- |
| Canonical interpreter/baseline | JavaScript semantics, effects and retirement | Always complete and authoritative |
| CFG/region plan | Blocks, edges, joins, liveness and backedges | Derived from immutable code facts |
| Generated artifact | Bytes, ABI, relocations, clobbers and entry | Published only after validation |
| Native admission | Guards, helper boundaries and exact fallback | Rejects before effects when proof is absent |
| Optimizing view | Derived dataflow and register choices | Never replaces semantic or ownership facts |

CFG liveness reaches a fixed point with a predecessor worklist; it has no
source-size iteration ceiling. Allocation failure is the only conservative
fallback, and that failure never changes canonical semantics.
Resident Bridge loops consume a sorted CFG-derived edge index with a
non-allocating lookup for backedges and transfers; diagnostic edge-list views
remain separate from the hot execution path.

The generated ABI contract marks which artifact rows are safe consumers of the
generic `NativeRegionContext`; today that is `Bridge`, `ArrayKernel`,
`ArrayNumericLoop` and `AffineI32Loop`. Typed-only kernels remain owned by
their plans. These consumers can execute proven scalar, control and tagged-word fragments and return to the canonical handler at a helper,
allocation, unsupported opcode, guard miss or publication failure. This is not universal
native opcode coverage or production OSR residency. The detailed admitted
families, CFG rules, constant contracts, loop polling, exit materialization and
fallback boundaries are specified once in the [JIT contract](stencil-jit-implementation-spec.md).

The canonical opcode declaration carries the generated Generic Bridge
eligibility bit in `OperationSpec` and the generated payload-family view used
by Bridge admission. Family overrides are declaration markers; dedicated
numeric rows derive their binary family from the same operator field. Runtime
admission adds only instance checks for flags, constants and physical
artifacts, after the shared `OperationSpec::generic_bridge_safe` effect/control
predicate; it does not maintain a second opcode allow-list.

The execution-profile JSON corpus is a canonical hot-IR contract, not a
physical-artifact expectation. The 342 records share one semantic target across
architecture modes; native-entry witnesses and machine-code measurements are
separate evidence. See [execution contracts](execution-contract-tests.md).

## Identity, activation and lifetime

Immutable code layout and mutable JavaScript identity are different facts.
`RegisterFile`, `Environment`, machine frames and generator state represent
active or suspended execution; shared cells preserve closure identity, TDZ,
mapped arguments, `eval` and `with` behavior. Arrays keep identity, element kind
and backing generation in one ownership record. Object layout, cache validity,
host roots and executable leases remain distinct authorities.

Object layout consumers validate the semantic layout together with the current
replacement representative through one owner-side guard; a matching layout on
a superseded object is not a valid native or inline-cache witness.
Native property consumers use the owner-side `has_current_layout` guard;
installation also rejects superseded representatives before publishing a
layout stamp. Native array consumers reject superseded representatives before
acquiring dense backing views.
Own-slot property/method, global, virtual, and named-write cache installation
applies the same replacement boundary before publishing a layout stamp, so
stale representatives cannot seed new cache entries.

`CallContinuation` is the canonical caller boundary: it owns the suspended
caller registers, code/PC, environment and result destination, and its shared
restore/delivery methods are used by specialized, fallback and tail-call paths.
Its caller range is validated against the immutable `CodeStore` before machine
resume. The cross-tier driver keeps suspended frames in one named iterative
activation stack; reserve failure restores the continuation window before the
canonical `RangeError` path. Call/construct argument storage uses the same
fallible boundary for large or spread argument lists. The completed cross-tier
authority owns helper/re-entry unification without a second completion
representation. Generator try-frame publication also goes through the
validated frame-stack boundary; no transition mutates the backing vector
directly or silently bypasses allocation failure. The production nested-call
stack uses the same fallible growth rule rather than a fixed guest-depth cap.

Every helper or re-entry edge names its roots, invalidatable pointers, live
state and completion transition. Mutable borrows are released before
native-to-Rust re-entry and backing views are reacquired after effects that can
resize, detach or collect them. Collector and cache changes must preserve
owning-edge multiplicity, weak references, suspended frames and active code
leases; they cannot invent a second frame, value or identity representation.
Dense native array stamps validate owner, backing generation, element kind and
replacement-representative identity together, so a structurally valid but
superseded array cannot keep a native view alive. Native array entry guards
reject superseded representatives before mutable backing acquisition; the
post-entry stamp still detects structural mutation during native execution.

## Publication and observation

`PhysicalInstallation` and `SharedPhysicalEntry` own artifact storage,
publication, leases, retirement and re-entry. Relocations, ranges, alignment,
helpers, CPU features and instruction-cache coherence are validated before an
image is callable. A failed publication leaves no partial entry, and an active
lease prevents reclamation of executing bytes. Compilation, cache and region
budgets protect compilation/publication and fall back atomically; they are not
JavaScript or architectural ceilings.

`execution_trace.rs` exposes opcode, slow-path, site, transition and lifecycle
observations. Counters are observations, not CPU time or proof of native entry.
The [evidence record](architecture-evidence.md) defines provenance and complete
aggregate rules; the [performance protocol](performance-lanes.md) defines
V8-v7, Bun/JSC and Node comparisons.

## Roadmap boundary

The [task ownership map](../tasks/README.md#the-ownership-map-is-deliberately-small)
assigns CFG/liveness and baseline retirement to the baseline CFG authority,
which owns CFG shape, liveness and baseline retirement; value/backing identity
to the completed value/backing authority; the completed cross-tier authority,
caches (093), reclamation (094),
optimization analyses to 095 and M4 policy to 097. This page describes current
consumers of those authorities; it does not create a second queue or claim that
the VM is already the fastest JavaScript engine.

The [native generation note](native-generation.md) explains how data-first Rust
macros and copy-and-patch artifacts inform generation without becoming a second
semantic model.
