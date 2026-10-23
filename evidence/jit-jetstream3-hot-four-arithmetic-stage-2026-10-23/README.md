# Arithmetic-stage JIT screen

Source: `v2-jit` revision `5bff15afeea0dac1d8b1d8b41b83d508019b0b82`, clean. Host: Linux x86_64, 4 vCPU quota, 16 GiB memory limit, rustc 1.99.0. JetStream corpus is recorded in `report.json`; Node 26.10.0 and Bun 1.4.2.

This is one instrumented round over the four hot fixtures, not a score qualification. All fixtures validated and Quench output equaled both peers. `quench-bench --analyze` correctly refused the report because diagnostic runs cannot be used for qualification. `--jit-diagnostics` adds overhead, and one round has no stable paired interval.

| Fixture | Quench | Bun | Node | Quench max RSS | JS native entries |
| --- | ---: | ---: | ---: | ---: | ---: |
| crypto | 65.95 | 510.43 | 31.40 | 32.93 MiB | 1,529 |
| delta-blue | 5.42 | 40.38 | 59.10 | 29.10 MiB | 278,307 |
| navier-stokes | 71.33 | 121.47 | 419.96 | 29.30 MiB | 0 |
| richards | 9.93 | 319.79 | 5.58 | 34.06 MiB | 0 |

Coverage counts match the prior diagnostic census, so this arithmetic-stage change did not expand hot-four native entry coverage. Scores vary substantially from earlier one-round screens; no performance win or regression attribution is made from this run.

Reproduce:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out target/jit-hot-four-after-arithmetic-stage.json
```
