# 341 — Memoized function loop fact

Status: complete

Stop rescanning `DynCode::blocks` on every monomorphic call merely to increment the
`native_loop_entries` statistic. Derive `has_loop` once when the immutable `DynJitCode`
image is built and read the boolean thereafter. The block list remains the canonical
control-flow representation; the boolean is its memoized projection.

All 124 release tests pass. Against Task 340, the five-pair 300 ms full comparison in
`reports/task341-memoized-loop-fact-full-ab-5/comparison.txt` measures 2193.87 ->
2228.99, **+1.60%**, with every component nonnegative. The decisive compound five-pair
500 ms comparison against Task 336 in `reports/task341-compound-full-ab-5-long/` measures
2183.02 -> **2234.06 (+2.34%)**. Richards improves 5.67%, DeltaBlue 5.39%, and the worst
component is Splay at -0.30%.

Accepted binary: `/tmp/deegen-task341-memoized-loop-fact`, SHA-256
`de0b3039d36bd48960478b0fa403ff3ad17851e592417d7d936ae976db532b55`.

