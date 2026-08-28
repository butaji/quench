# Benchmark integrity

Benchmarks measure the ordinary Node-compatible VM; they never select its
semantics. Production code must not inspect fixture/source identity, markers,
scores, checksums, or another engine, and may use only reusable proven or
guarded facts with complete fallback semantics. Instrumentation is outside the
measured execution path. Accept a change only after independent Node comparison
of values, descriptors, identity, ordering, errors, exit status, and host
effects.
