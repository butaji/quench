# 32-probe adaptive native field-site trial

Candidate: per-PC field adaptation samples 32 reads and enables native-PC continuation only when at least 29 hit. The production PGO build trained on the general runtime corpus, then ran 11 interleaved rounds against the saved control binary from the same PGO source revision. Outputs matched. Linux RSS is diagnostic.

| Workload | Candidate ops/s | Control ops/s | Median change | Paired-round wins | Candidate RSS change |
|---|---:|---:|---:|---:|---:|
| React SSR | 1,290.0 | 1,335.8 | -3.43% | 0 / 11 | +4.79% |
| Zod parse | 88,495.6 | 88,105.7 | +0.44% | 6 / 11 | -3.30% |
| Pino JSON logger | 51,020.4 | 51,020.4 | 0.00% | 3 / 11 | -3.91% |

This setting is also rejected. React regressed in every paired round, while Zod's gain was below 1% and Pino was flat. Both this and the 8-probe setting are diagnostic screening only; all adaptive native-PC source changes were reverted. The current production binary was restored to the original PGO control (`0fd1ddc4…`).
