# Dedicated compiled-lane `GetIterator` trial

The candidate assigns `GetIterator` a dedicated verified-bytecode lane handler and invokes the existing guarded `Vm::get_iterator` implementation directly. It avoids decoding and redispatching through `Vm::step`; custom iterator lookups, calls, and errors continue through the shared semantics.

The candidate reused the existing general-purpose PGO profile, which was generated without scored npm workloads.

Candidate SHA-256: `e74d8611310123ba768670088eb1e79267d8fea92832ae2db150cc4194f5da23`

Control SHA-256: `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`

Each JSON report contains 11 interleaved candidate/control JIT rounds with matching output and checksums.

| Workload | Candidate ops/s | Control ops/s | Throughput change | Paired wins | RSS change |
|---|---:|---:|---:|---:|---:|
| React SSR | 1,315.79 | 1,342.28 | -1.97% | 1/11 | -4.94% |
| Zod parse | 88,105.73 | 87,336.24 | +0.88% | 8/11 | -4.57% |
| Pino JSON logger | 51,020.41 | 51,546.39 | -1.02% | 3/11 | -4.94% |

Decision: reject. Results do not show a consistent material throughput improvement and do not meet the project goal. RSS remains a process-tree sampler diagnostic and is not treated as qualified peak-memory evidence.
