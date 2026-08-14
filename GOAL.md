# Goal: stages 41–45

Bring test262 stages 41–45 to 100% passing through the canonical runner:
BigInt, Boolean, DataView, Date, and DisposableStack. Preserve completion
ordering, realm identity, constructors, prototypes, descriptors, coercions,
accessors, proxies, and user-observable errors. Fix shared runtime semantics,
not individual tests; do not alter `tests/test262` or the harness. Re-run
stages 41–45 plus all earlier completed regressions after each fix. Finish with
zero failures, clean format/clippy checks, a focused regression guard when
needed, and a committed verified change.
