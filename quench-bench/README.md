# Benchmark harness rules

The harness measures Quench through the same `quench-node` host path used by
ordinary programs.

- Keep benchmark fixtures and runners outside production runtime semantics.
- Do not add fixture detection, score/checksum logic, alternate engines, or
  workload-specific implementations to the host or VM.
- Build tracing, profiling, and symbolized binaries separately so measurement
  instrumentation cannot perturb the measured artifact.
- Treat benchmark output as diagnostic evidence only. Node-compat tests remain
  the acceptance authority.
