# Per-operation measurement rules

- Keep microbenchmarks bounded, deterministic, and representative of reusable
  operations rather than named benchmark fixtures.
- Report time, memory, allocation proxies, and missing or timed-out samples
  explicitly; never turn unavailable data into a passing zero.
- Use measurements to find regressions and representation costs, not to select
  runtime semantics.
- Every measured operation must remain observable-equivalent to the ordinary
  Node-compatible VM path.
