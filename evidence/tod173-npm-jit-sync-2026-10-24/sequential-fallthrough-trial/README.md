# Sequential continuation after JIT callouts

This candidate changed the shared general JIT's VM-step fallback. When the
callout returned the statically expected next PC, generated code branched
directly to that instruction block instead of entering the indirect PC
dispatch continuation. Nonzero status and nonsequential control flow retained
the existing paths. The change was tested with a fresh production PGO profile
trained only on the general runtime curriculum.

Candidate SHA-256: `088295a0357bc0e3dd7772ddf960e102a920153b322c80a6a9ce57335a17b487`

Control SHA-256: `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`

Each report contains 11 interleaved candidate/control JIT rounds. Outputs and
checksums matched. Linux RSS is sampled process-tree diagnostic data only.

| Workload | Candidate ops/s | Control ops/s | Change | Paired wins | Fastest peer gap |
|---|---:|---:|---:|---:|---:|
| React SSR | 1,352.45 | 1,326.61 | +1.95% | 9/11 | -96.75% |
| Zod parse | 85,470.09 | 88,105.73 | -2.99% | 1/11 | -90.60% |
| Pino JSON logger | 50,505.05 | 51,020.41 | -1.01% | 3/11 | -87.88% |

Decision: reject and revert. The optimization had inconsistent workload impact
and did not materially reduce the large peer throughput gaps.
