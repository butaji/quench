# Activation architecture: shared semantics, specialized storage

## Decision and evidence

Recommended direction for Quench, not a claim of universal optimality: one logical
activation contract consumed by interpreter, native regions, calls, suspension,
root traversal and diagnostics. Keep different physical representations where
lifetime requires them. Extend existing Machine/RegisterWindow/Environment types;
do not introduce another runtime or semantic graph.

Current code has Machine registers/environment/frames/call_frames, Environment
slot/name/TDZ state and pooling, and GeneratorState suspension/private/async state.
These mechanisms exist; the gap is their shared ownership/layout contract, not
absence of a stack or a need to rename everything. Several code ranges and resume
destinations are carried in parallel representations. required_register_count
still walks nested frame fragments at entry. The architectural requirement is
that static layout disappears before execution and dynamic state has one owner.

Measured motivation: Richards' frame-clear samples and call-shape counterfactual;
EarleyBoyer's repeated width traversal and heavy environment traffic; NavierStokes'
383M local reads and108M register copies despite packed numeric storage. See
`target/stencil-review-1788646448/v8-priority-matrix.md`. Events are not CPU shares;
these findings motivate the design, not a quantified speedup prediction.

## Alternatives

| Approach | Advantage | Main cost / constraint |
| --- | --- | --- |
| Heap environment and tagged slots for every call | Simple capture/suspension integration | Allocation, indirection, RC traffic and retained unused state; pooling does not remove these semantics |
| Native machine stack as the only activation | Excellent conventional call locality | Stable Rust cannot promise custom tail ABI; suspension, reentry, roots and native exits require substantial machinery |
| VM-owned activation storage with selective escape/suspension | Shared interpreter/native layout, bounded ordinary calls, explicit roots | Needs verified layout/liveness, capture promotion and relocation-safe references |

Choose the third direction incrementally. A bounded segmented slot arena is a
candidate, not an unconditional requirement: compare against existing pooled
windows on call-heavy workloads before replacing storage. Segments avoid moving
active native pointers during growth; indices/handles also permit moving storage
if all pointers are invalidated and reacquired at boundaries. Do not keep a Rust
borrow or raw backing pointer valid across arbitrary reentry by assumption.

## Single logical contract

- Immutable per-function descriptor: frame width, operand roles/argument windows,
  captured and dynamically observable bindings, control/handler descriptors and
  safepoint/exit liveness. Derive from canonical lowered operations, not syntax.
- Active record: code-owner lease, code ID/PC, slot window, receiver/new.target/
  arguments state, dynamic handler phases and caller result continuation.
- Non-escaping locals: direct frame slots, not one allocation per local. Tagged
  canonical slots at generic boundaries; native interiors may hold unboxed values.
- Escaping bindings: shared cells only where identity/sharing is required. Closure
  capture stores the declared cells/values, not an unnecessary entire caller chain.
  TDZ, mapped arguments, direct eval and with retain their required observability.
- Suspended record: owned live slots plus pending control frames, completion and
  lexical capabilities. Transfer or retain storage according to actual aliases;
  copying a snapshot must not fork shared binding identity. Do not retain native
  machine registers as the sole copy of suspended state.
- Resume and native exits: one result contract identifies continuation, completed
  effects, live locations and completion kind. Storage shared by await/yield does
  not make their language semantics interchangeable.

Root maps and diagnostic views derive from these same declarations and live
records. Static ownership inside CodeStore uses IDs/ranges; escaping functions,
activations and native leases retain the store. Avoid cycles from immutable code
back to its own strong owner. Code, heap and executable lifetimes remain distinct.

## Native performance contract

Compatible logical frames do not mean storing every intermediate after each
instruction. Leaf/fused/loop stencils retain values in registers across their
supported interior; materialize required tagged/root state at observable exits,
calls and safepoints. Share slow semantics and reconstruction data across tiers.
Fixed register roles and bounded local spills are acceptable; unknown alias/effect
facts end a region or require guards, not speculative cross-call reuse.

[Sparkplug](https://v8.dev/blog/sparkplug) demonstrates the integration benefit of
interpreter-compatible frames and native-PC/bytecode mapping. Borrow that contract,
not its decision to mirror every interpreter register store. Quench's desired
region residency requires stronger physical specialization inside safe boundaries.
[V8 call-frame work](https://v8.dev/blog/adaptor-frame) illustrates why argument
adaptation belongs in a coherent call layout, not layers of redundant wrappers.

## Migration and evidence

1. Freeze layout metadata and document actual ownership; remove repeated static
   discovery without changing JS behavior or adding a second authoritative table.
2. Unify logical entry/exit/root/suspension contracts behind existing adapters.
   Remove superseded state only after all consumers use the canonical owner.
3. Compare direct frame slots/selective captures and bounded storage candidates
   against pooled environments. Preserve a complete dynamic-observation path.
4. Compose native regions against that layout; prove live-outs and reentry before
   measuring dispatch, boxing and retain/release reductions.

Acceptance is architectural: ordinary non-escaping calls need no per-binding heap
allocation; frame-width lookup is constant-time; suspended bytes track retained
live state rather than whole unrelated stacks; shared captures preserve identity;
native interiors do not synchronize every intermediate unnecessarily. Measure
allocation/bytes/copies, startup, steady state, RSS and code footprint separately.
Test eval/with/mapped arguments, recursion, mutual calls, retained closures,
suspension, exceptions and native-to-host reentry as contract coverage. Counts
must use consistent physical populations; existing lifecycle counters are not
yet a valid allocation-minus-drop live gauge.

Task075 owns required integration/ownership contracts. Task073 owns performance
experiments and storage changes beyond that gate; do not expand075 into an
unbounded activation or GC rewrite. Individual defect diagnosis belongs to Codex.
