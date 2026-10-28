# Generic JS JIT entry work floor experiment

The experiment tested two generic bytecode-work floors in `js_compiled.rs` to keep tiny bodies on the lane. Floor 2 assigned two units to `Unary`, `Binary` and `GetField`, one to branches. Floor 6 assigned six to `Unary`, four to `GetField`, two to `Binary`, and one to branches. Both are independent of source or benchmark identity.

Source revisions were clean `2dc7cee6d78615449b790ce8a6e85c0e2fcc5bfc` (floor 2) and clean `22e8c15c74e63fa9f9c1bc16fd36d5f6cd4c9beb` (floor 6). All reports use the stock JetStream 3 TypeScript-lib fixture and passed output validation. Bun was 1.4.2; Node was v24.19.0; Linux x86_64 with 4 CPUs and a 16 GiB cgroup limit.

| Floor | Quench JIT score / RSS | Bun JIT score / RSS | Node JIT score / RSS | Quench round wall times |
| --- | ---: | ---: | ---: | --- |
| 2 | 0.19 / 186.4 MiB | 1.60 / 398.4 MiB | 1.81 / 348.2 MiB | 27.45, 28.88, 32.01 s |
| 6 | 0.19 / 187.7 MiB | 1.40 / 397.0 MiB | 2.48 / 351.7 MiB | 29.31, 30.88, 24.76 s |

This is a three-round screen per revision, not suite qualification. Quench's median score did not change between floors, and the host peers varied substantially between the runs. The diagnostic samples are instrumented and not timing comparisons. Floor 6 reduced compiled helpers for some low-work predicates, but did not reduce total code bytes or show a matched score gain. The admission-floor source changes were reverted after this result.

## Diagnostic snapshots

Floor 2: 43 compiled functions, 11,080,728 native entries, 176,128 code bytes, 99.1 ms compile time in the one-round sample.

Floor 6: 41 compiled functions, 8,792,461 native entries, 167,936 code bytes, 103.2 ms compile time in its one-round sample. `isLineBreak` remained compiled with 3.87M native entries; several tiny equality helpers did not appear in this sample. The screen does not show a score improvement.
