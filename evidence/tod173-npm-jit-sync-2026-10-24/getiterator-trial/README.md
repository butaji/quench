# Direct `GetIterator` callout trial

This candidate was built with the general PGO training corpus on 2026-10-24. The scored npm packages were not used to train the PGO profile. It adds a `GetIterator` branch to the compiled JavaScript callout loop and delegates to the same `Vm::get_iterator` implementation used by normal dispatch.

Candidate SHA-256: `8ee2facfb95b84ad252636af8fadaa59515020d04b1363dfd7c076ebcdb38ec0`

Control SHA-256: `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`

Each saved report is an interleaved 11-round JIT candidate/control comparison. All paired outputs and checksums matched. Median candidate throughput regressed on all three packages; measured RSS improved, but that does not offset the throughput loss.

| Workload | Candidate throughput | Control throughput | Change | Paired wins | Candidate RSS | Control RSS | RSS change |
|---|---:|---:|---:|---:|---:|---:|---:|
| React SSR | 1,326.26 ops/s | 1,337.26 ops/s | -0.82% | 3/11 | 26,533,888 B | 28,082,176 B | -5.51% |
| Zod parse | 86,956.52 ops/s | 88,888.89 ops/s | -2.17% | 3/11 | 22,024,192 B | 23,212,032 B | -5.12% |
| Pino JSON logger | 50,000.00 ops/s | 52,083.33 ops/s | -4.00% | 3/11 | 20,750,336 B | 21,909,504 B | -5.29% |

Decision: reject and revert the source change. The benchmark host's RSS sampler is a Linux process-tree diagnostic, not a portable peak-memory qualification measurement. This result does not meet the project goal.
