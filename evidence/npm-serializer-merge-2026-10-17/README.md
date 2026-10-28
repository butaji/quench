# V8 serializer, advanced IPC and the v2-merged merge, 2026-10-17

Host: Linux 6.18.44 x86_64 (cloud VM), 4 vCPU Intel Xeon @ 2.10GHz, 15 GiB RAM.
Oracle: official Node v26.12.0.

`ffb052f91` ports V8's ValueSerializer (structuredClone, postMessage, `node:v8`,
`serialization: 'advanced'` IPC) and gives stdio writes Node's encoding and callback
behaviour. `434f56ece` merges v2-merged `67e00ee6f`: SharedArrayBuffers now use the Node
lane's `SharedBacking`, and `cares_wrap` is one binding (c-ares channel when linked, the
pure-Rust channel otherwise) under Node's lib/dns.js.

## Results

- `vitest-scenario-ffb052f91.json.gz`: vitest@5.0.4 is behavior-pass. The forks pool
  needed advanced IPC and an empty `process.stdout.write('', cb)` callback.
  The previous full run (DNS build) had 284 behavior-pass, 13 declarations, 2 Quench
  failures (vitest, minizlib) and 1 oracle failure.
- `node_contracts` (dns, v8 serdes, advanced IPC, stdio write), `worker_threads`,
  `napi_addons` and `shared_regressions` all pass on 434f56ece.
- `child-entry-434f56ece.txt`: 458 passed, 13 failed. Eight of the failures are the
  known environmental ones. The other five also fail on v2-merged itself: four crypto
  inventories/providers depend on the host OpenSSL, and the warning-listener source check
  conflicts with d796e622d's lazy listener.

## Instruction guard

`instruction-guard.txt`: iteration-profile builds of v2-merged `67e00ee6f` (base) and
434f56ece (candidate). Per-run work changes by -0.17% to +0.01% across all eight benches.
Startup rises by about 2.8M instructions (0.1-0.2%), from the additional native
bindings registered at bootstrap.
