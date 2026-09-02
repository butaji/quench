# Benchmark integrity

Benchmarks measure the ordinary Node-compatible VM; they never select its
semantics. Production code must not inspect fixture/source identity, markers,
scores, checksums, or another engine, and may use only reusable proven or
guarded facts with complete fallback semantics. Instrumentation is outside the
measured execution path. Accept a change only after independent Node comparison
of values, descriptors, identity, ordering, errors, exit status, and host
effects. Workload-specific kernels and recognizers are not part of
`quench-runtime`; reusable fact-guarded kernels may serve any JavaScript
program and must retain complete fallback behavior. Admission may depend on
runtime state such as value tags, object layout, or builtin identity, but never
on a fixture's name or source shape. Benchmark harnesses remain measurement-only.

This extends to every guard, quickening site, and cache degrade tier: their
admission rule and bound must generalize to any JavaScript program with the
same runtime shape (same value tags, same shape/property diversity, same
callee-identity diversity). A kernel is general-purpose when its threshold
was chosen for that generality, not because it happens to make one known
benchmark's specific shape count, call count, or loop trip count fast. Sizing
a bound to a known benchmark's numbers, even without naming the benchmark, is
the same violation as inspecting its fixture name.
