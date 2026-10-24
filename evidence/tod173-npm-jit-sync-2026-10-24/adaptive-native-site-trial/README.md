# Adaptive native field-site trial

Candidate: an opt-in JIT adaptation window per field PC. The runtime samples eight cache probes; after at least seven hits it allows native-PC continuation at that site. Other sites keep exact VM stepping. The field cache remains the guard; accessors, proxies, misses, and exceptions use the existing VM semantics.

The candidate was built with the general PGO corpus only (no scored npm workloads). It was compared in 11 interleaved rounds against the saved control built from the same PGO source revision and corpus. Both binaries produce matching workload outputs. Linux RSS is diagnostic.

| Workload | Candidate ops/s | Control ops/s | Median change | Paired-round wins | Candidate RSS change |
|---|---:|---:|---:|---:|---:|
| React SSR | 1,321.4 | 1,331.6 | -0.77% | 1 / 11 | -5.44% |
| Zod parse | 83,333.3 | 86,956.5 | -4.17% | 5 / 11 | -6.23% |
| Pino JSON logger | 51,020.4 | 50,505.1 | +1.02% | 6 / 11 | -8.25% |

This version is rejected: it regresses React and Zod throughput, and the small Pino gain does not justify broadening the change. The clean PGO control binary was restored locally after the trial. The candidate source change is reverted; per-site counters remain for later work. These are screening results, not qualification results.
