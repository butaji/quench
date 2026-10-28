# Broad native field-site marking trial

Candidate change: mark every general-dispatch `GetField` as eligible for the existing cached-field callout probe. The runtime source change was reverted after the matched screens.

The production build used a clean general PGO corpus (no scored npm workloads); candidate binary SHA-256: `7fd642a0fdfeeca54ad8977a74927ce22b8ec62b2c2b9fe25a3291399d46357f`. The runner interleaved the candidate, saved control binary, Node, and Bun for 11 rounds. All outputs matched. RSS remains diagnostic on this Linux host.

| Workload | Candidate ops/s | Control ops/s | Candidate vs control | Candidate RSS | Control RSS |
|---|---:|---:|---:|---:|---:|
| React SSR | 1,314.8 | 1,331.9 | -1.3% | 25.32 MiB | 25.73 MiB |
| Zod parse | 88,495.6 | 80,971.7 | +9.3% | 21.63 MiB | 23.28 MiB |
| Pino JSON logger | 51,546.4 | 56,818.2 | -9.3% | 18.78 MiB | 20.46 MiB |

This broad change is rejected: the Zod gain did not generalize, while React and Pino regressed. The raw runner reports preserve peer rates, commands, sample distributions, input hashes, and outputs. No change from this trial remains in the runtime.

Next profiling should identify hot `GetField` sites and their existing cache behavior before another implementation. Any next optimization must avoid taxing cold field sites and compare across all three workloads.
