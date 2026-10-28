# Adaptive hot-site GetIterator callout

This rejected candidate counted `GetIterator` callouts by program, function,
and bytecode PC. After 256 hits at a site it called the existing
`Vm::get_iterator` semantics directly and cached the decision in the general
dispatch native-site bitmap. It kept the ordinary VM operation for cold sites
and checked frame storage before resuming native execution. The production PGO
profile was generated from the exact candidate source; the profile-use build
reported no function hash mismatches.

Candidate SHA-256:
`44f127af7abf4682b56600b97c5640389fc40559658d5e7467bf80a4f58a5de0`

Control SHA-256:
`0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`

Each report contains 11 interleaved JIT rounds with matching output and
checksums. The fastest-peer comparison uses Node and Bun medians from the same
report. Linux RSS is sampled process-tree diagnostic data only.

| Workload | Candidate ops/s | Control ops/s | Change | Paired wins | Fastest peer gap | RSS change |
|---|---:|---:|---:|---:|---:|---:|
| React SSR | 1,346.98 | 1,321.70 | +1.91% | 9/11 | -96.77% | -6.73% |
| Zod parse | 79,365.08 | 80,321.29 | -1.19% | 5/11 | -92.86% | -5.02% |
| Pino logger | 50,505.05 | 50,000.00 | +1.01% | 6/11 | -88.89% | -6.02% |

Decision: reject and revert. The speed changes are small and inconsistent,
while the three packages remain far behind peers. The RSS values are not
qualification evidence.
