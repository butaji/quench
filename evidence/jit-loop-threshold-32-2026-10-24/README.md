# JS loop threshold 32 diagnostic · 2026-10-24

This is a three-round selected-fixture screen of the production binary built from source revision `8c07ff80051465491ccd30a26e907c437b3041dc` plus an uncommitted experiment lowering `LOOP_TIER_THRESHOLD` from 1,000 to 32. Both reports say `source_dirty=true`, `fixture_set_matches=false`, and `qualification_ready=false`; this is diagnostic evidence, not a score claim. Host: Linux x86_64, four logical CPUs/four-CPU quota, 16 GiB memory limit. Node v26.12.0 and Bun v1.4.4.

## Results

| Fixture | GeneralDispatch | Quench | Node | Bun | Quench/Node | Quench/Bun | Quench RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| web-ssr | off | 17.46 | 90.22 | 103.81 | 0.194x | 0.168x | 113.7 |
| web-ssr | on | 13.57 | 86.78 | 97.81 | 0.156x | 0.139x | 118.0 |
| hash-map | off | 21.42 | 278.81 | 303.73 | 0.077x | 0.071x | 36.4 |
| hash-map | on | 17.84 | 260.22 | 305.50 | 0.069x | 0.058x | 38.0 |

Both fixtures were valid and output-equal in all three rounds. On this binary, the opt-in GeneralDispatch mode scored 16.7% below `--jit` on hash-map and 22.3% below it on web-ssr. Neither mode approaches the 1.20x gate.

## What the threshold unlocked

The one-round instrumented hash-map report shows the `run` function compiled by GeneralDispatch: 120 entries, one successful general compile, 7,984 code bytes, 5.38 ms compile time, 2,603,874 native entries. It accumulated exactly 342,400 work units, its priced compile threshold. No loop OSR region compiled. Its native body made 2,513,993 field-cache probes (2,423,626 hits) and recorded millions of user-call frame pushes: 1,973,503 at one site, 450,001 at another, and about 90,000 at two more sites. The hot callees `computeHashCode` and `_findKeyEntry` each had 2.42 million entries but no native entries; `equals` had 450,150 entries and no native entries.

Lowering the threshold unlocked a large native parent region, but the VM callouts still send its hot callees through interpreter frames. This is a native-entry increase without a matched score improvement. The threshold change is reverted. Next target: a general call continuation/target-cache path that preserves `Vm::step` semantics and resumes the caller at its exact successor, measured against these per-site call counts and end-to-end scores.

## Commands

```sh
target/production/quench-bench --fixtures web-ssr,hash-map --suite jetstream3 --jit --jit-js-general-dispatch --runs 3 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/node-v26.12.0-linux-x64/bin/node --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true --out target/jit-loop-threshold-32-2026-10-24/on.json

target/production/quench-bench --fixtures web-ssr,hash-map --suite jetstream3 --jit --runs 3 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/node-v26.12.0-linux-x64/bin/node --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true --out target/jit-loop-threshold-32-2026-10-24/off.json

target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit --jit-js-general-dispatch --jit-diagnostics --runs 1 --timeout-ms 600000 --quench target/production/quench-node --node /workspace/node-v26.12.0-linux-x64/bin/node --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true --out target/jit-loop-threshold-32-2026-10-24/diagnostics.json
```
