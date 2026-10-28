# Cached `this` field return screen

This is an unpaired, one-round JetStream 3 `hash-map` screen on the Linux
x86_64 cloud host. The baseline executable is the production binary built from
`51e32ab44`; the changed executable contains an initial cached-`this`
field-return matcher. Both rounds passed JetStream validation and had equal
output. Follow-up diagnostics showed that the initial matcher recognized none
of the hot `get key` or `get value` calls, so these timings cannot measure the
intended optimization and are retained only as a record of the failed screen.

| Build | Quench Score | Quench peak RSS | Quench wall time |
| --- | ---: | ---: | ---: |
| Baseline | 1.02 | 73.82 MiB | 292.93 s |
| Initial matcher (zero fast entries) | 4.46 | 73.89 MiB | 108.88 s |
| Corrected matcher | 3.76 | 74.13 MiB | 130.98 s |
| Narrow four-word matcher | 3.75 | 74.06 MiB | 124.85 s |

These are unpaired one-round screens, not evidence of a measured optimization.
The Bun and Node scores varied substantially between separate runs. Only the
corrected and narrow matcher rows had confirmed fast entries, but peer
variation makes the Quench score delta inconclusive. The narrow sample still
scored far below Bun (241.64) and Node (193.62).

The corrected candidate recognizes a return-marked `GetField` from fixed
`this`, with no frame handlers, then selects a later ordinary `Return` opcode
for VM unwinding. It returns only on the existing cached data-field hit.
Accessors, proxies, cache misses, and non-fixed `this` continue through the
interpreter from bytecode zero. The corrected diagnostic run recorded
10,799,999 fast entries each for `get key` and `get value`, out of 10.8 million
candidate entries; one initial cache miss per function used the interpreter.
JetStream output validation passed. The report is
`corrected-matcher-diagnostics-1round.json`.

Reproduction (replace `target/production/quench-node` with the baseline path
for the baseline report):

```sh
target/production/quench-bench --fixtures hash-map --suite jetstream3 --jit \
  --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true \
  --out target/jit-field-return.json
```
