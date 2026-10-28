# Benchmark harness

Fixtures and scoring are measurement-only; they never select VM behavior.

- [V8-v7 commands](../docs/v8_v7.md)
- [JetStream 3 commands](https://linear.app/todoxapp/document/v2-jet-score-mem-status-and-measurement-contract-ae88abad6a7f)
- [Micros manual](micros/README.md)
- [Backlog (Linear)](https://linear.app/todoxapp/project/quench-abb744c79c4e/overview)

Run measurements only after the affected execution path passes its correctness
checks. Every report must retain source, binary, toolchain, and host identity.
Use a new output path for each report so evidence is never overwritten.

### Verify frozen instruction-guard inputs

`verify-fixed-work` checks the exact frozen binaries and N=0/N JavaScript files
from an instruction guard. It retains stdout, stderr, exit status, completion
markers and before/after SHA256 identities. Use a fresh output directory:

```sh
cargo run -p quench-bench --bin verify-fixed-work -- INPUT_DIR REPLAY_OUT BASELINE CANDIDATE
cargo run -p quench-bench --bin verify-fixed-work -- --callgrind INPUT_DIR COUNT_OUT BASELINE CANDIDATE
```

The first command checks execution only. The second records fresh matched
Callgrind counts, including each instrumented process's completion, and reports
startup-subtracted instruction deltas. Keep the original input paths. New counter
runs capture stdout to files and must use their own freshly measured baseline;
older piped-output counts are separate artifacts.

Exit status validates measurement data. The 1% instruction gate additionally
requires `report.json`'s `instruction_limit_met` to be true. Regressions remain
explicit in `comparisons`; replay success does not qualify throughput or RSS.
