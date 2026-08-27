# Performance-lane rules

Execution lanes are diagnostic views of the same Node-compatible VM. They do
not select semantics or dispatch benchmarks.

- Build tracing and sampling artifacts separately from measured binaries.
- Keep counters, symbols, and reports outside the ordinary execution path.
- Measure reusable operations and state transitions, not named workloads.
- Join measurements by binary, profile, and run identity; unavailable metrics
  remain unknown rather than becoming zero.
- Treat lane ratios, samples, allocations, cache data, and scores as evidence
  for engineering decisions, never as compatibility results.
- A lane or microbenchmark must exercise the ordinary VM and preserve the same
  output, errors, ordering, exit status, and host effects as Node.
- A measurement failure is fixed by correcting measurement or general runtime
  behavior, never by adding a benchmark-shaped fast path.
