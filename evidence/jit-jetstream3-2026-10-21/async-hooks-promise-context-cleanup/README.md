# Doxbee Promise context cleanup

The Doxbee allocation profile traced a retained ~544 MiB `HashMap<u64, i64>`
to async-hooks Promise trigger contexts. A context-trigger entry was created for
each Promise even when hooks were not observing it, and dead Promise IDs were
not removed from that map. Promise async IDs also share a numeric representation
with private job-context tokens, so collection now tracks those lifetimes in
separate sets and maps. Full GC supplies the live Promise IDs; dead entries are
removed from `context_triggers`, and an oversized sparse map is shrunk.

The fix was measured in an uninstrumented production build with JetStream 3,
JIT enabled, three rounds, and matching fixture output:

| Fixture | Quench score | Median max RSS | Previous three-round RSS | RSS reduction |
| --- | ---: | ---: | ---: | ---: |
| `doxbee-promise` | 4.35 | 170,123,264 bytes | 1,011,986,432 bytes | 83.2% |
| `doxbee-async` | 5.83 | 257,544,192 bytes | 1,052,766,208 bytes | 75.5% |

The previous scores were 3.69 and 4.69. This screen is sequential rather than
paired, so score changes are not conclusive; the RSS reduction is large and
consistent with the allocation trace. Node and Bun scores varied between the
screens, so no comparative score claim is made. The Quench scores remain far
behind both peers.

The build was normal production (`cargo build --profile production -p
quench-node -p quench-bench`) with no profiling feature enabled. Reproduction:

```sh
target/production/quench-bench \
  --fixtures doxbee-promise,doxbee-async \
  --suite jetstream3 --jit --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node --node node --bun bun \
  --out production-3round.json
```

The raw report records the dirty source state based on revision
`59dc8907efbeda43ee633681ea9b88f33621380e`, the corpus revision, executable
hashes, per-round measurements, and output validation.
