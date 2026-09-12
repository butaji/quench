# 346 — Loop-header native-entry table

Status: planned

Derive one immutable `bytecode_pc -> NativeEntry` table when a function image is linked.
Every loop header, backedge target, and other explicitly exported internal entry records
the native address plus its typed connector context. This is first-execution stencil
infrastructure: it does not count iterations, choose a tier, or defer compilation until
a path becomes hot.

`NativeEntry` is data, not an alternate control-flow implementation. Its canonical
fields are the bytecode PC, image-relative offset, entry tag, connector-schema identity,
and frame-state identity. Absolute addresses are derived only after Task 348's image
placement is final. Tasks 146/181 may transfer directly to a compatible entry; Task 22 may later
use the same table for OSR without inventing another program-point map.

All entry tags, maximum exported entries, offset widths, and validation failures use
named types or constants. A duplicate PC/tag or connector mismatch is a link error, not
a fallback to the interpreter.

Acceptance: every CFG backedge target has one matching native entry; forward and backward
entry lookups are tested; relocation and image movement preserve relative offsets;
incompatible connector states are rejected; the default execution path remains
stencil-only and contains no runtime hotness branch.
