# Bounded array-length construction screen

This experiment tried two related changes: route the fast-lane `new Array(number)` case through the interpreter's shared constructor operation, and build bounded hole-only arrays directly in dense storage rather than creating and discarding sparse metadata.

The controlled V8-v7 Richards screen compares binaries built from the same base revision, `0fffe7768`:

- baseline SHA-256: `2690fac580f24754d36cf5dc1f60324ae247679a8c637931bf284972a63d540e`
- candidate SHA-256: `e1ac21e24fba62126c1c03ff1f581a929349631a970b8050a91e9a2d5ff294eb`
- 11 valid fixed-work rounds; outputs equal in every round; Linux did not provide IPC/cycle counters, so no round met the runner's complete cleanliness checks.
- Candidate/baseline paired geometric mean of marginal work time: `1.059` (candidate 5.9% slower); paired median ratio: `1.107`; candidate faster in 2/11 rounds.
- Median maximum RSS: baseline `12,066,816` bytes; candidate `12,034,048` bytes, effectively unchanged.

The source experiment was removed. A prior comparison against a binary predating the base merge had appeared 6% faster; this exact-base comparison supersedes that contaminated screen. The raw report is `richards-nojit-pair-11.json`. This is a one-fixture no-JIT screen, not JetStream qualification and not evidence about JIT mode.
