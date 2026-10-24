# JS JIT tier-admission screen · 2026-10-24

This is a 3-round JetStream 3 screening run from `e51e55b7c9add08ab6f1917703974794aefd42b2`, on Linux x86_64 with four logical CPUs, a four-CPU quota, and a 16 GiB memory limit. Production binaries were built with `cargo build --locked --profile production -p quench-node -p quench-bench`. The report records `source_dirty=false`; both reports are marked `qualification_ready=false` because they contain only three rounds and three selected fixtures.

Quench ran with `--jit`; the `on` report also passed `--jit-js-general-dispatch`. Bun 1.4.4 ran with `BUN_JSC_useJIT=1`; Node v26.12.0 was probed to confirm Maglev/TurboFan compilation. The runner preflight passed. All selected fixtures produced valid, output-equal results.

| Fixture | GeneralDispatch | Quench score | Node JIT | Bun JIT | Quench/Node | Quench/Bun | Quench RSS MiB | Node RSS MiB | Bun RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Web-SSR | off | 12.43 | 55.48 | 69.43 | 0.224× | 0.179× | 113.9 | 214.9 | 225.6 |
| Web-SSR | on | 9.80 | 52.60 | 78.85 | 0.186× | 0.124× | 118.0 | 214.9 | 228.8 |
| hash-map | off | 16.24 | 185.98 | 205.15 | 0.087× | 0.079× | 36.7 | 172.6 | 104.9 |
| hash-map | on | 19.62 | 225.24 | 287.98 | 0.087× | 0.068× | 36.7 | 160.4 | 94.4 |
| Crypto SHA-256 | off | 30.66 | 247.29 | 434.31 | 0.124× | 0.071× | 35.2 | 59.5 | 53.7 |
| Crypto SHA-256 | on | 31.20 | 220.05 | 373.62 | 0.142× | 0.084× | 35.2 | 60.1 | 53.4 |

Quench is far below both JIT peers on all selected workloads, though its measured process RSS is lower. The on/off runs were sequential three-round screens, and peer scores also shifted between runs; they do not establish a causal GeneralDispatch effect. These results do not meet the 1.20× score target or the 11-round qualification requirement. The instrumentation checkpoint itself has no performance claim.

Raw reports:

- `gendispatch-off.json`
- `gendispatch-on.json`
- `callsite-diagnostics.json` — one-round diagnostic run; its instrumented timings are not score evidence.

The diagnostics run used commit `c2b9dd38c15f301ba8fbce3f17637cb25cb976ac`, `--jit --jit-js-general-dispatch --jit-diagnostics`, and one round each of `hash-map` and `web-ssr`. Hash-map had 195 function rows, one compiled function, 2,129 native entries, and no observed native call-site rows: its hot `run` function remained `general-promotion-pending` after 120 entries, with four admitted loop sites and no OSR compile attempts. Its five-instruction `runIteration` wrapper also had 120 entries but no native entries; its one specialized compile attempt was rejected. Web-SSR had 2,142 function rows, four compiled functions, and 216,564 native entries; seven dynamic eval call sites were seen once each. The interpreter timers are inclusive and overlap, so the large `run`/`runIteration` totals are not exclusive CPU attribution.

Commands, from the repository root:

```sh
target/production/quench-bench --preflight-only --suite jetstream3 --jit --jit-js-general-dispatch \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true

target/production/quench-bench \
  --fixtures web-ssr,hash-map,stanford-crypto-sha256 \
  --suite jetstream3 --jit --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true \
  --out target/jit-tier-admission-call-stats-screen-2026-10-24/gendispatch-off.json

target/production/quench-bench \
  --fixtures web-ssr,hash-map,stanford-crypto-sha256 \
  --suite jetstream3 --jit --jit-js-general-dispatch --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true \
  --out target/jit-tier-admission-call-stats-screen-2026-10-24/gendispatch-on.json

target/production/quench-bench \
  --fixtures web-ssr,hash-map --suite jetstream3 --jit \
  --jit-js-general-dispatch --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true \
  --out target/jit-tier-admission-call-stats-screen-2026-10-24/callsite-diagnostics.json
```
