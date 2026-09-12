# 102 — Reject unrepresented AOT stencil relocations

Status: complete

Copy+patch is sound only when every address-dependent machine-code field is represented
as data. During the [[32-element-kind-guarded-arrays]] coarse-run experiment, LLVM
lowered opcode dispatch to a local jump table and floating remainder to an external
`_fmod` call. The extractor silently ignored both relocations, copied unresolved bytes,
and produced an `Illegal instruction` in Crypto.

Compile AOT handlers with the named rustc `-Cjump-tables=no` option. During catalog
extraction, collect every in-range relocation and fail the build unless its target is
one of the explicit next, slow, or taken-branch holes. Floating remainder is excluded
from the inline-run capability and its unrepresentable standalone stencil is not
exported until external-call patches exist. The run handler funnels both branch forms
through one lexical tail-call site, preserving the catalog's single branch-hole shape.

This is an edge-effect rule in the stencil category: an extracted leaf is closed over
machine code plus its declared hole obligations; hidden linker state is forbidden.
All 42 release tests pass, `otool -rv` reports only declared hole relocations in the
cooked object, and `reports/relocation-guard-smoke.jsonl` passes all eight V8v7 suites.
