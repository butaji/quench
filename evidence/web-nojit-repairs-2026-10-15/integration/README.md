# Final shared integration

After publication of the original repair set at f7967e8f3, v2-merged advanced to
d11a93411 with a separate promise-resolution change. Merge 5258b123b retains both
histories. This directory qualifies that combined source: 617 runtime tests,
five Node v26.12.0 differential regressions, the new promise-order/species test,
the workspace/all-target check and all nine no-JIT framework combinations pass.

The original instruction-count evidence remains tied to runtime source
310810075 and its exact recorded executable hash; it is not silently retargeted
to this later integration binary. No production speed or memory claim is made.
The known HTTP/API gaps remain open, full Node parity is not established, and
optimization remains blocked. All source and measurement identities are retained.
