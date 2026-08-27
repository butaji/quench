# Benchmark integrity rules

Benchmarks measure the Node-compatible runtime; they are never runtime inputs
or semantic authorities.

- `quench-node` contains only Node host/API behavior. It must not recognize
  benchmark filenames, source text, suite markers, scores, checksums, or
  another engine.
- `quench-runtime` may use only general-purpose optimizations admitted from
  reusable IR facts. A failed or unknown guard returns to complete VM
  semantics.
- Production code must not contain workload-shaped kernels, benchmark-only
  constants, alternate runtimes, or score/checksum gates.
- Performance instrumentation and benchmark harnesses remain outside the
  execution decision path and must not change observable behavior.
- Accept optimizations only when Node-compat behavior is independently tested
  for values, calls, properties, mutation, exceptions, coercions, allocation,
  ordering, exit status, and host effects.
