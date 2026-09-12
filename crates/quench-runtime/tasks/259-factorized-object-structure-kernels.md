# 259 — Factorized object-structure kernels (layout × prototype)

Status: planned

Replace the two per-object metadata words `ShapeRef` and
`Option<HeapRef<ObjectCell>>` with one immutable shared `StructureRef`. A structure is a
hash-consed product kernel:

`Structure = LayoutShape × PrototypeIdentity × RepresentationFlags`

`LayoutShape` remains independently interned so objects with the same ordered fields but
different prototypes still share property descriptors and transition edges. `Structure`
interning shares the complete product among objects with the same layout/prototype facts.
An object stores the structure identity plus its inline/out-of-line values; prototype
mutation and property-layout mutation select another immutable structure instead of
mutating shared metadata.

This is one categorical product object, not a new execution tier. Own-property stencils
project the layout component, inherited-property/`instanceof` stencils project the
prototype component, and a fully refined IC may guard the product with one identity
comparison. Kernels are immutable and shared; only object-local value storage and the
copy-patched IC/stencil instance are mutable/per-site.

The experiment must begin with a model, because the tradeoff is real. It saves one word
per object and may collapse shape-plus-prototype guards, but prototype lookup gains a
descriptor indirection and the number of structure kernels can approach the product of
layouts and prototypes. Record object counts, distinct layouts, distinct prototypes,
distinct product structures, own/inherited guard counts, and estimated/live bytes on all
V8v7 suites before changing representation. Use named constants for structure IDs,
representation flags, and any inline cache capacity.

Acceptance: structural interning and transition tests; `Object.setPrototypeOf`/prototype
replacement, deletion/dictionary, constructor, inherited property, and `instanceof`
correctness; one-word per-object metadata demonstrated by layout assertions; at least one
representative IC disassembles to a single structure guard; complete semantic smoke;
resident-memory and alternating V8v7 A/B improve. Reject the representation if the extra
indirection or structure-product cardinality outweighs the saved object word/guard.

Primary sources: V8 HiddenClasses and transition trees
<https://v8.dev/blog/fast-properties>; JavaScriptCore's compact structure identity and
shared immutable/hash-consed structures
<https://webkit.org/blog/10308/speculation-in-javascriptcore/> and
<https://webkit.org/blog/7846/concurrent-javascript-it-can-work/>.

