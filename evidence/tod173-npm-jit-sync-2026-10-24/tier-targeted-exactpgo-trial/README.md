# Targeted dispatch work sampler

The candidate samples interpreter dispatch work only on frames already admitted to the general-JIT tier, flushes a bounded sample when a frame is recycled, and stops tracking once native code is installed. The compile price was unchanged. PGO was trained on the general runtime curriculum, not the scored npm workloads.

All three reports used candidate binary SHA-256 `9542785da0779aceae7eab7f4868e19d2161411c9c40386514aa3ef4e4aed7af` and control SHA-256 `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`. Each ran 11 matched interleaved rounds with output parity.

| Workload | Throughput vs control | Paired wins | Quench JIT throughput | Fastest peer throughput |
|---|---:|---:|---:|---:|
| React SSR | -0.6% | 5/11 | 1,333 ops/s | 41,322 ops/s |
| Zod parse | +1.8% | 9/11 | 88,496 ops/s | 1,111,111 ops/s |
| Pino JSON logger | 0.0% | 4/11 | 51,546 ops/s | 454,545 ops/s |

This is a useful work-attribution mechanism but does not close the peer gap. Linux RSS is diagnostic only. Broader sampling of all hot legacy candidates is documented separately in `../tier-hotcandidate-trial/` and was reverted.
