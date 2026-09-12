# 95 — Inline binary dispatch into the semantic kernel

Status: complete

Test LLVM-forced inlining of `exec_op_ref`, the canonical binary operation dispatcher,
into the already-inlined semantic block kernel. Release symbol inspection shows it
still survives as a call for every `DynOp::Binary`. Inlining should remove a high-volume
call boundary and let LLVM share tag/operand loads with the outer bytecode dispatch.

The operation semantics remain defined exactly once by `define_ops!`; no bytecode
pattern or benchmark identity participates. Because this may duplicate a sizable
switch into several immutable kernel variants, record executable-size and full-suite
performance, and revert on a failed gate.

Rejected at the smoke gate: forcing the whole binary dispatcher inline increased the
release executable from the task-94 2,974,224-byte build to 2,991,696 bytes. The full
smoke in `reports/binary-inline/smoke.jsonl` fell sharply: DeltaBlue 406→263,
Earley-Boyer 1373→933, RegExp 230→156, and Splay 1830→1081 relative to task 94's
balanced medians. The attribute was reverted without spending a longer A/B cycle.
Inlining copied cold string/coercion/equality paths into every block-kernel variant and
damaged instruction-cache/code layout. A future attempt must isolate a compact numeric
fast path and leave generic semantics out of line.
