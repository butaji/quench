# 129 — Guarded numeric property-slot view

Status: complete

Define a stable `repr(C)` view for direct property-region stencils: retained receiver
owner, expected immutable shape identity, fixed slot, and raw property-value address.
Entry setup resolves the existing monomorphic/prototype IC data, verifies that the
receiver and holder shapes still match, and requires a numeric current value. A direct
region may borrow that one-word number without an `Rc` increment because its typed
context forbids escape and retains the receiver/holder for the region lifetime.

Existing-slot numeric writes preserve shape and vector capacity. Any missing key,
shape transition, prototype mutation, heap-valued slot, call, allocation, deletion, or
escaping register rejects the view and enters the canonical semantic block. Property
names participate only in cold slot resolution; the copy-patched instance carries view
indices, not strings.

Acceptance: layout assertions, own/inherited invalidation tests, no-escape analysis,
numeric read/write agreement, owner lifetime tests, and explicit rejection of structural
mutation. The view is derived per invocation from immutable region requirements and
does not add hotness detection.

Implemented `PropertySlotView` as one `repr(C)` pointer word plus a retained
`GuardedProperty`. Validation walks immutable shape identities through the prototype
holder, resolves one fixed slot, requires a numeric immediate, and rejects inherited or
missing writes. The guard can audit its shape chain and exact backing pointer before use;
existing numeric writes preserve shape and allocation. Tests cover own load/store,
prototype shadowing, prototype relinking, receiver-graph lifetime, heap rejection, and
non-object rejection. All 67 release tests and the complete smoke in
`reports/task129-guarded-property-slot-view-smoke.jsonl` pass.

The infrastructure-only binary is `/tmp/deegen-task129-guarded-property-slot-view`,
SHA-256 `7fbe3ebdc9ccd7289cf96d5ae60dc98cc6af5eb1cd5f8f110310420fdd07310f`.
No performance claim is made before Task 130 consumes the view. The no-escape proof is
enforced by Task 130's quoted-region capability predicate; this task supplies the
runtime side of that contract.
