# TOD-196 stage 1: lazy-codegen capture facts predicted from free names

Every plain function of a replayable source in the 26 JavaScript tests of the JetStream 3
default set (one iteration each, cold: `QUENCH_COMPILE_CACHE=`), on a `verify-replay` build with
`QUENCH_LAZY_FACTS_LOG=1`: the captures predicted from the function's free names, resolved
through the scopes it closes over, equal those of its compiled subtree for all 29,379
functions (`verdicts-by-test.txt`; the JetStream harness and Node's library modules count in
every test).

The check first found compiler-synthesized lookups of `TypeError`, `ReferenceError`, `Object`
and `DisposableStack` that read the scope instead of the realm's intrinsics: a semantic bug,
fixed in 723d26047. Names the source declares nowhere count as free (an eval's surrounding
bindings); an undeclared `arguments` is the nearest non-arrow function's own.
