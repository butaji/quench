# Doxbee glibc trim threshold screen

Three-round JetStream 3 screens compared the normal Quench launcher with an
otherwise identical wrapper setting `MALLOC_TRIM_THRESHOLD_=0`. Both used the
same production `quench-node` binary, `--jit`, and the same Node/Bun peers. The
wrapper changed no runtime source or GC behavior. Outputs matched for both
fixtures in all rounds.

| Fixture | Quench Score, default → trim | Quench median peak RSS, default → trim | Trim/default RSS |
| --- | ---: | ---: | ---: |
| `doxbee-promise` | 3.69 → 3.83 | 1,011,986,432 → 1,011,990,528 bytes | 1.0000 |
| `doxbee-async` | 4.69 → 4.89 | 1,052,766,208 → 1,052,696,576 bytes | 0.9999 |

The apparent 3.8–4.3% Score gains are not conclusive: peer timings varied
substantially between the sequential screens, and this experiment was not
paired or randomized. RSS did not change. Do not enable aggressive allocator
trimming based on this result. Continue investigating Doxbee's memory through
measurements that separate live runtime data, allocator retention, and native
or JIT code memory.

Raw reports: `default-3round.json` and `trim-threshold-zero-3round.json`.
