# 342 — Identical source-context elision experiment

Status: complete

Test whether same-source nested calls should avoid pushing and popping the same source ID.
The candidate stored whether each frame changed source context, pushed only on a change,
and popped conditionally. Cross-source semantics and error attribution remained intact.

The five-pair 500 ms comparison in
`reports/task342-source-context-identity-full-ab-5-long/comparison.txt` is neutral and
mixed: 2146.60 -> 2149.04, **+0.11%**. The saved vector operations were offset by another
sidecar state word and conditional cleanup. The candidate was rejected and reverted.

Rejected binary: `/tmp/deegen-task342-elide-identical-source-context`, SHA-256
`69dae4db00e5aa5f66a42dbec3f80f81a2229632131ff0ef9edec5074604f3bb`.

This is further evidence against polishing individual sidecar operations. Erase the
complete call boundary through Task 20 or Task 146 instead.

