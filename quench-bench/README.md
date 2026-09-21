# Benchmark harness

Fixtures and scoring are measurement-only; they never select VM behavior.

- [V8-v7 commands](../docs/v8_v7.md)
- [Micros manual](micros/README.md)
- [Task queue](../tasks/index.json)

Run measurements only after the affected execution path passes its correctness
checks. Every report must retain source, binary, toolchain, and host identity.
Use a new output path for each report so evidence is never overwritten.
