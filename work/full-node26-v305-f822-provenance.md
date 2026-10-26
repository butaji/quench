# Node v26 `parallel/` census v305 (f822)

- Branch/source: `v2-cloud-node` / `f822fc199700bad816d189e9011d3f39737807d5`; tracked source clean, `tests/node` submodule dirty.
- Runtime: Node `v26.12.1`, `/workspace/work/node-v26.12.1-linux-x64/bin/node`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Built runner: `/tmp/tod20-v164-session-target/release/run-parallel`, SHA-256 `d0a09b8e43fd69a0b0b330f263b7cce5e8a0cc48e997593c0d2a541c71772053`.
- Fixture root: `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`, 5,023 names, inventory hash `47b65dd4945a6e57`.
- Gate parameters: 5 workers, 60s timeout, Linux x86_64; all 5,023 fixture results present.
- Results file `work/full-node26-v305-f822-results.json`, SHA-256 `386f292c03527efd125b61d49dbccdd9f6a549e307005c1494f8b4b4811c0c9e`; checkpoint `work/full-node26-v305-f822-checkpoint.jsonl`, SHA-256 `616b5148795908585109b2522af775da030ba94834dd255e8cc538e786d5459a`.
- Full counts: pass=3517, skip=650, fail=684, timeout=160, crash=8, unclassified=4.
- Frozen baseline v167 source `0a91368ec9a7e92df103a40dda4d3c40833d5271`: pass=3388, skip=650, fail=784, timeout=196, crash=5, unclassified=0.
- Fixture-paired transitions: retained pass=3312, non-pass to pass=205, baseline pass lost=76, net pass change=129. Matrix: crash->crash 4, crash->fail 1, fail->fail 613, fail->pass 169, fail->timeout 2, pass->crash 3, pass->fail 63, pass->pass 3312, pass->timeout 6, pass->unclassified 4, skip->skip 650, timeout->crash 1, timeout->fail 7, timeout->pass 36, timeout->timeout 152.
- Unclassified: 4 successful exit-0 `process.execve()` fixtures have no worker JSON: test-process-execve-on-exit.js, test-process-execve-permission-granted.js, test-process-execve-socket.js, test-process-execve.js. They are not credited as passes.
- TAP classification uses the runner rule: `GuestExit` with guest code 0 and stdout prefix `1..0 # Skipped:` is skip; guest code 0 otherwise pass; nonzero guest exit is fail.
- Acceptance open: 3,517 passes, 483 short of 4,000; post-trunk full gate remains required.
