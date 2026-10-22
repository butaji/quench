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

## Quench vs JIT peers

With the default enabled, the harness completed a separate valid 3-round
JetStream report for the same fixture (`peers-3round.json`, all outputs equal).
Quench median Score was 2.83 / 73.99 MiB, Bun 1.4.3 was 165.97 / 83.56 MiB,
and Node 24.19.0 was 124.87 / 222.77 MiB. Quench used less RSS than both
peers, but its Score was only 1.7% of Bun's and 2.3% of Node's. This is a
three-round screen rather than the 11-round qualification, and it demonstrates
that the local +11.4% A/B improvement is far from the requested score win.

## Cached native `equals` entry experiment (rejected)

A follow-up cached the already-compiled two-number Boolean `equals` entry for
the bounded lookup loop, holding its `Rc` owner while calling the raw native
entry. A diagnostic run validated stock output and recorded 53,999,969 native
`equals` entries. The clean same-binary A/B then ran twelve alternating arms
in two windows; raw records are `direct-ab-*.json`.

| Arm | Median Score | Median sampled RSS | Median wall time |
| --- | ---: | ---: | ---: |
| Existing `js_jit_compiled_call` | 2.515 | 74.19 MiB | 169.0 s |
| Cached raw native entry | 2.385 | 74.23 MiB | 174.9 s |

Only 3 of 6 adjacent score pairs and 2 of 6 time pairs favored the raw entry.
The six-pair median score ratio was 0.950 and median time ratio was 1.128;
RSS rose by 0.03 MiB. The raw-entry optimization was reverted. This rejects
that call-boundary shortcut for this fixture; it does not alter the previously
screened default numeric lookup path.
