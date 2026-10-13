# Splay direct constant-array allocation (M4)

The candidate creates constant-array bytecode results directly in the existing `ArrayRecord` representation, avoiding a temporary generic `Cell::Array`/`Object` conversion. The generic path remains for arrays with arbitrary properties and for the exact fallback.

The isolated production candidate binary is `6ad304cf3fdb121555f0350671bb222215c149460d9e4f1d5bc69df4c450f987`; the parent binary is `e0ac4367fc95b6f8d95dfc9ce1e2a958cbfbdf72462fe381e0c20ce70eb9d6fa`. The source was dirty at measurement time: checkout `216cc2a0e3b412cd051362b48ba07a5df92e4713`, diff SHA-256 `80b60132481e9c3f7d6750f57368f9301b270573bf8e9d9e128a8551d3233b06`. The measurement runner SHA-256 is `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`. Raw schema-5 reports and their hashes are listed in `summary.json`.

The dedicated Splay A/B and the all-eight Splay row both improve marginal wall time. In the all-eight paired report, Splay wall is -1.386% (95% interval -2.445% to -0.169%); max RSS is 0.152% relative to the parent. The active wall interval excludes zero. Non-active wall and RSS median deltas remain inside the TOD-11 limits after subtracting the same-binary A/A upper interval. Richards has 29 clean A/B pairs due its RSS top-up; every other fixture has 11.

Correctness: the full runtime suite passed (557 unit and 6 integration tests), `cargo check --locked -p quench-node` passed, and Test262 Array stage 31 passed 3,081/3,081 while ArrayBuffer stage 32 passed 221/221. Test262 Object/Promise/Proxy/Reflect stages 60–63 also passed 4,604/4,604, and the pinned shared-VM Wasm suite passed 67,124/67,124. There is no production benchmark-identity branch.

The M4 stock Score and max-RSS retake is still required before treating this package as a qualification improvement. It was held because the host load rose to 17.90 on 10 logical CPUs and Docker/FleetAPI workers were active. Splay remains the only active fixture and is not yet won.


A stock 11-round campaign completed on the candidate, but is excluded: Deno test workers and unrelated Bun workers appeared, and one-minute load reached 3.90 on 10 logical CPUs (0.390/CPU). Diagnostic medians were Quench 9,879 vs Node 14,945 Score and Quench 65,257,472 B vs Bun 64,012,288 B max RSS; these do not update the distance. The excluded raw report is `stock-attempt-1.json`.
