# Wasm JIT follow-up experiments

These reports are diagnostic, one-round screens from a dirty worktree at
`e64538e072a89c6b0fe7b85904565ff332fd5acf`. They are preserved to show how
the fast-path changes and the rejected coverage expansion behaved. Every
report validated all selected fixtures and recorded equal output, but none is
qualification evidence (`qualification_ready` is false).

## Fast-path screen

`one-round-pretrap/jetstream3-wasm-jit.json` measures the source after fusing
the Wasm indirect-call helper, adding same-pool signature identity checks, and
removing the common indirect-call signature heap clone. Quench remains below
both JIT peers on Score in all five fixtures. It uses less RSS than both peers
for gcc-loops and quicksort, and less than Node for HashSet; it does not win
both metrics on any fixture.

| Fixture | Quench JIT Score / RSS MiB | Bun JIT | Node JIT |
| --- | ---: | ---: | ---: |
| HashSet | 32.46 / 113.1 | 220.09 / 55.5 | 225.38 / 126.9 |
| gcc-loops | 76.21 / 48.0 | 187.81 / 55.5 | 187.33 / 91.8 |
| quicksort | 172.27 / 41.9 | 351.87 / 49.4 | 425.97 / 88.5 |
| tsf | 13.61 / 224.7 | 175.87 / 136.3 | 173.65 / 139.4 |
| zlib | 12.42 / 112.8 | 85.75 / 104.3 | 85.60 / 110.8 |

The separate TSF diagnostics in `one-round-pretrap/tsf-jit-diagnostics.json`
recorded 156/244 native functions, 177,800 emitted bytes, 180,224 mapped
bytes, about 116.7 ms compile time, and 2,816,456 compiled-to-VM callouts.
This points away from executable mapping size as TSF's main RSS cost.

## Reverted eager coverage experiment

`reverted-unreachable/` preserves the screen and diagnostics from temporarily
adding native lowering for Wasm `unreachable`. It compiled all 244 TSF
functions, but grew emitted code from 177,800 bytes to 9,226,568 bytes and
compile time from about 117 ms to about 3.04 s. The one-round TSF Score fell
from 13.61 to 7.61 and RSS rose from 224.7 MiB to 281.4 MiB; the attempt was
reverted. These screens do not establish a statistically significant causal
effect, but the diagnostic growth is direct evidence against eager admission
of all newly eligible functions. The compiler lowering and its test are not
part of the current source.

The raw reports can be reproduced with the commands below after building the
production binaries and setting the pinned Node and Bun paths. `--qjs
/bin/false` satisfies the runner's unused QuickJS version probe for this
JetStream lane.

```sh
fixtures=HashSet-wasm,gcc-loops-wasm,quicksort-wasm,tsf-wasm,zlib-wasm
target/production/quench-bench --fixtures "$fixtures" --suite jetstream3 --jit \
  --runs 1 --timeout-ms 600000 --quench target/production/quench-node \
  --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/one-round-pretrap/jetstream3-wasm-jit.json
```

The reverted experiment used the same command with the experimental build and
output path `reverted-unreachable/jetstream3-wasm-jit.json`; its diagnostics
were collected separately with `--jit-diagnostics`.
