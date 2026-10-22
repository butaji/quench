# First npm compatibility baseline, 2026-10-14

This is the first 100-package representative-scenario baseline of the expanded
300-package/function-parity task in TOD-127. It does not prove whole-package parity.
The ranking is current npm weekly downloads among 4,665 discovered candidates,
not an exhaustive registry census. `ranking-source-current.json` retains all
observations and five seed-query exceptions. `current-inventory.json` and
`current-package-lock.json` identify the tested versions and transitives.

Latest published Node v26.11.1 passes all 100 scenarios. Quench passes 91 runtime
API/CLI scenarios and three declaration-consumer scenarios. Six fail: source-map
(JavaScript WebAssembly API), Zod and yargs (cyclic ESM instantiation), esbuild
(Worker threads), whatwg-url (synchronous require of ESM), undici (HTTP/2 surface).
`candidate.json` preserves every process observation; the runner returns nonzero.
Native CLI checks and declaration consumers are separately identified, and this
run is Linux x86-64 only. No upstream-suite or all-functions coverage is claimed.

The report records a dirty candidate based on f9bdbab. Its executable hash is
authoritative; the compatibility source is retained in the accompanying commit.
The current snapshot differs from the earlier `initial-*` inventory and
`baseline.json`; do not combine those observations with the current results.

Workspace/all-target checks pass, as do 589 runtime library tests and the added
latest-Node regression checks. Global formatting, clippy and boundary checks have
pre-existing failures (unchanged formatting, quench-stack unit-error return types,
runtime Test262 references); those are not reported as passing gates.

The all-eight Callgrind guard compares f9bdbab and the candidate with deterministic
fixed work, subtracting N=0 startup from N-run instruction counts. The command is:

```sh
PATH=/workspace/.local/usr/bin:$PATH \
VALGRIND_LIB=/workspace/.local/usr/libexec/valgrind \
bash evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh \
  /workspace/scratch/npm-base /workspace/scratch/npm-candidate-publish \
  target/npm-instruction-guard-publish
```

`instruction-guard-raw.txt` and `instruction-guard.txt` retain all 32 counts and
the per-run differences: between -0.04% and +0.01%, below the 1% hold threshold.
`guard-completion.json` independently verifies exit zero, exact completion output
and empty stderr for all 32 runs. Both builds use the iteration profile; this is
an instruction-regression screen, not a production performance measurement or
speed claim. `host-and-build.json` records executable hashes, platform and cgroup
limits. Build/check/test logs are retained alongside the observations.
