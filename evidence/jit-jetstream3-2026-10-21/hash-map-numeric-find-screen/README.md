# Guarded numeric hash-map lookup A/B

The existing entry helper for `HashTable::_findKeyEntry` recognized the exact
23-instruction loop and performed guarded inline-cache probes plus a guarded
compiled `equals` call. Diagnostics showed it returned natively from the VM
helper on 53,999,968 of 54,000,000 entries, with only the final entry falling
back. This screen checks the helper against ordinary bytecode interpretation.

Both arms ran the stock JetStream 3 `hash-map` fixture through the same
production `quench-node --jit` binary. The only difference was
`QUENCH_JS_JIT_NUMERIC_FIND=0` or `1`. The six score runs alternated in this
order: disabled, enabled, enabled, disabled, disabled, enabled. Each result
records raw stdout, process exit, binary hash, source head, wall time and a
20-ms `/proc/<pid>/status` `VmHWM` sampler. Stock output validation passed on
all six runs. Run reports are `ab-*.json`.

| Arm | Scores | Median Score | Sampled peak RSS | Median wall time |
| --- | --- | ---: | ---: | ---: |
| Disabled | 2.63, 1.88, 2.72 | 2.63 | 74.06 MiB | 174.6 s |
| Enabled | 3.07, 2.60, 2.93 | 2.93 | 74.17 MiB | 163.9 s |

The three adjacent score pairs all favor enabled (1.17x, 1.38x, 1.08x).
The enabled median score is 11.4% higher, sampled median RSS is 0.15% higher
(0.11 MiB), and median wall time is 6.1% lower. This three-pair, one-host
screen is sufficient to stop keeping the narrow path opt-in, but it is not
11-round qualification and does not establish a win over Node or Bun. Bun is
not installed in this workspace. The earlier opt-in diagnostic run is stored
as `enabled-diagnostic-1round.json`; it is excluded from timing comparisons.

Host: Linux x86_64, Rust 1.99.1, Node 24.19.0. JetStream checkout:
`06785cf861ac44855f168cbbe829278c2802e6de`. Production binary SHA-256:
`0aa131af9c23c31dd50e1811be31e7985daa0500b6440c5b5641c0d7a411bf3e`.

After changing the default, a fresh production build was run with the toggle
unset. The stock fixture exited successfully and scored 2.00 with sampled peak
RSS 74.16 MiB on that single noisy round. This confirms default-mode execution
and output validation, not a repeat of the A/B estimate. Its binary hash and
raw output are in `default-clean-1round.json`.
