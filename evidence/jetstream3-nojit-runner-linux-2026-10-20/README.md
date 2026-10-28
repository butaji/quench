# JetStream 3 no-JIT, canonical runner, 2026-10-20

As [2026-10-19](../jetstream3-nojit-runner-linux-2026-10-19/README.md) with one round per
test, from `v2-cloud-jet` at a296419e. The container restarted between
the two runs and Bun's own geomean rose 13%, so compare ratios: over the 33 tests Quench and
Bun both completed, the geomean is Quench 8.79, Bun 18.46 (ratio 2.10; 2.25 on 2026-10-19).
