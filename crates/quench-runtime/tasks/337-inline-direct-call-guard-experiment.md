# 337 — Inline monomorphic direct-call guard experiment

Status: complete

Re-evaluate Task 181's direct-call stencil after Task 336 removed repeated child-frame
construction. The initial unchanged experiment remained negative on its four call-heavy
suites: median Richards 866 -> 852, DeltaBlue 881 -> 837, RayTrace 1928 -> 1875, and
Splay 3376 -> 3355. Runtime evidence showed why: the cooked direct-call stencil invoked
the Rust helper before checking its IC, including 1,305,125 RayTrace misses among
2,425,513 attempts.

The candidate moved the readiness and exact-identity guard into the rustc/LLVM-cooked
stencil. A shared `repr(C)` schema derived matching runtime and AOT target layouts, so the
stencil could read the published recipe pointer, callee register, and identity word
without magic offsets. Capturing targets kept a null fast-path recipe. The successful
helper then consumed the guard proof rather than repeating bounds, target, identity, and
capture branches. Empty, polymorphic, native, and capturing sites rejoined the canonical
slow stencil without first crossing the direct-call Rust helper.

This correctly removed helper misses: diagnostic runs reported equal helper attempts and
hits. It still did not make the tiling profitable. The distinct-binary, three-pair,
300 ms complete comparison in
`reports/task337-inline-direct-call-guard-rejected/comparison.txt` measured **2174.78 ->
2162.03 (-0.59%)**. Richards improved 1.42%, RayTrace 0.37%, Earley-Boyer 0.31%, and
Splay 1.39%, but DeltaBlue fell 2.97%, Crypto 3.34%, RegExp 1.09%, and Navier-Stokes
0.64%.

Baseline Task 336 SHA-256:
`453322a3a669547a49d5fd5851363954cde7937d3da14a6224be080b8d666bde`.
Rejected candidate SHA-256:
`12f8c0719bab78945667ba304d3c3d2f0a6484b2c803cf3f2e2f1177d9e0c101`.

The candidate was reverted. A deterministic release rebuild exactly reproduces the Task
336 baseline hash, and all 124 release tests pass. The experiment sharpens the boundary
diagnosis: IC readiness and identity checks are not the remaining problem. Splitting a
generic block around each call still replaces one block-kernel entry with multiple host
entries. Do not retry this form. A successful call optimization must either inline the
callee quote or transfer directly to a guest-stack callee/return continuation without a
host helper.
