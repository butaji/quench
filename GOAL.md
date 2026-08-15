# Goal: stages 46–47 and 82–94

Bring test262 stages 46–47 and 82–94 to 100% passing through the canonical
runner: Error, FinalizationRegistry, decodeURI, decodeURIComponent, encodeURI,
encodeURIComponent, eval, global, isFinite, isNaN, parseFloat, parseInt, and
undefined. Preserve completion ordering, coercion, descriptors, realms,
direct-eval semantics, URI validation, finalization behavior, and exact
observable errors. Fix shared runtime semantics rather than individual tests;
do not alter `tests/test262` or the harness. Re-run these stages plus earlier
completed regressions after each fix. Finish with zero failures, clean
format/clippy checks, and committed verified changes.
