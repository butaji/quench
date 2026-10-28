# Read-file supplied buffers and read-sync parity

Frozen candidate `b5f76d523` passed 282 serial API differentials against official Node 26.12.0, 626 runtime tests, workspace/all-target checking, 43 framework fixtures and all nine no-JIT web correctness targets. The runtime check uses the identical Core source from integration 700bf9ba5. The earlier combined supplied-buffer snapshot passed 279 API tests before the three additional read-sync/unknown-size tests.

Repairs preserve caller buffer aliases and factory receiver, descriptor ownership and positions, native byte copies independent of public setters/metadata, options and flags getter ordering, readSync overload/zero-length rules and BigInt positions, intrinsic native-binding errors, and real Unix positional-read errno without changing the shared offset. Error.captureStackTrace primitive target diagnostics also match stable Node.

Counterexamples preserve the failed read-sync matrix and unknown-size buffer/native errno probes. They motivated the final repairs; their earlier binaries are not this qualified candidate. A later application internal-module filename collision is separate work and excluded from this frozen snapshot.

These checks do not establish total Node parity or a throughput win. Remaining async filesystem, descriptor writes/lifecycle, workers, HTTP/2, Buffer pooling and clone host protocols remain open. Fresh paired Callgrind artifacts are added only after capture completes; their performance gate is reported independently from correctness.

All 32 strict paired Callgrind executions completed valid work with unchanged binary/input identities. The 1% instruction gate fails on DeltaBlue (+5.9231%); the other seven rows pass, including Earley-Boyer (+0.6639%). Capture uses a fresh matched baseline and candidate with regular-file output. The verifier repository revision records the working checkout at capture completion; candidate identity is the binary hash tied to b5f76d523 above, and explicitly excludes subsequent 62116bd54. This is instruction evidence only, not throughput, RSS or isolated attribution.
