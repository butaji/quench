# General JIT admission price trial

The candidate lowered the estimated general-baseline compilation cost to one quarter of its prior value to test whether frequently entered pending functions could repay compilation earlier. The existing 50% savings assumption remained in force. It reused the general PGO profile and was compared with the same interleaved 11-round JIT control.

Candidate SHA-256: `874b8a9bb7b9d369d7ba19139d4ad72c6f6b0b9ed6edcf2587e38eeb2c12d391`

Control SHA-256: `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`

| Workload | Candidate ops/s | Control ops/s | Throughput change | Paired wins | RSS change |
|---|---:|---:|---:|---:|---:|
| React SSR | 844.45 | 1,349.89 | -37.44% | 0/11 | +4.58% |
| Zod parse | 65,573.77 | 85,470.09 | -23.28% | 0/11 | +3.86% |
| Pino JSON logger | 50,505.05 | 51,546.39 | -2.02% | 2/11 | -1.66% |

All three workload outputs matched. Decision: reject and restore the previous cost model. The earlier promotion gate was conservative, but a fourfold lower price triggers compilation that is substantially more costly than its measured savings on React and Zod. This screen does not meet the performance goal; RSS remains diagnostic.
