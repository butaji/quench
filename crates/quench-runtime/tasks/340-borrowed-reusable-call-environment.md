# 340 — Borrowed reusable-call environment

Status: complete

Remove the `Rc<Environment>` retain/release pair performed by every warm noncapturing
monomorphic call. `UserCallIc` already owns the environment, and a reusable activation
already owns the same environment after its first construction. Pass a borrowed `&Env`
through `call_with_reusable_activation`; clone only on the allocation path.

This is an identity erasure at the function-morphism boundary, not a new environment
representation. Capturing calls retain the canonical path.

All 124 release tests pass. The five-pair 500 ms complete comparison in
`reports/task340-borrowed-call-environment-full-ab-5-long/comparison.txt` measures
2159.39 -> 2197.65, **+1.77%**. Richards improves 1.06%, DeltaBlue 2.65%, RegExp 2.21%,
and Splay 8.90%; the worst component is Navier-Stokes at -0.86%.

Accepted binary: `/tmp/deegen-task340-borrowed-reusable-call-environment`, SHA-256
`57aa16c880e3101bd526b1101fea58a203ea24c33a788e8a11d602505c8eab61`.

