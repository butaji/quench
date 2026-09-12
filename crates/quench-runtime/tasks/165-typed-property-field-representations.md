# 165 — Typed property-field representations

Status: planned

Extend immutable shapes so every fixed property descriptor carries a storage
representation such as `I32`, `F64`, `TaggedWord`, or `HeapRef`. Shape transitions widen
representations monotonically. A store that no longer fits exits to one shared migration
kernel, creates or finds the widened successor shape, converts the slot, and updates the
object; compatible reads and writes use representation-specific stencils directly.

Typed field facts flow through Task 152 contexts. Within a guarded region, a property
load yields the declared representation without another tag test, and a compatible
store stays raw until a join or canonical side exit requires materialization. Constants
and tags use named definitions; no representation is encoded by unexplained numbers.

Acceptance: migration preserves JavaScript semantics and shape sharing; old shapes remain
valid until lazily migrated; differential tests cover integer overflow, doubles, `NaN`,
`-0`, heap references, prototypes, and deletion; shape-propagated property loops remove
repeated tag checks and improve relevant full-suite components.

An F64 field load/store must have an FPR-native Task 158 connector variant. Do not load
the raw bits into a GPR, box them, store them into the virtual register array, and then
reload into an FPR in the next arithmetic stencil. JavaScriptCore's direct-double
GetByOffset/PutByOffset lowering is the concrete model; its implementation specifically
targets removal of the GPR/FPR round trip: <https://commits.webkit.org/298092@main>.

Sources: <https://arxiv.org/abs/1507.02437> and
<https://v8.dev/blog/react-cliff>.
