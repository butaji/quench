# Current V8-v7 JIT native-coverage diagnostic

This reruns the four stock V8-v7 workloads against clean source revision
`2e728c23ec40f16c44f79f8ec151442110732ee1` with the production
`quench-node` binary SHA-256
`9b0b1e9621d3a3200db35d59b08e0d625409db4b23cf0ca16b442b8085418f56`.
Inputs are the stock scripts archived in the sibling `v8-v7-native-coverage`
directory. Each completed with its own benchmark assertions. Scores and run
times are diagnostic output, not peer comparisons.

| Fixture | Native function entries | OSR region runs | General-dispatch instructions |
| -- | --: | --: | --: |
| Crypto | 5,404 | 8,113,117 | 47,370 |
| DeltaBlue | 37,491 | 0 | 2,193 |
| NavierStokes | 0 | 53,898 | 1,703 |
| Richards | 0 | 0 | 22,487 |

The new general-dispatch counters exclude lane and native instructions. They
are execution counts, not exclusive time. Richards remains the coverage gap:
`Packet` alone contributes about 20,230 general-dispatch operations from
constructor initialization (`Construct`, `SetThisFieldStrict`, `LoadConst`,
`Return`); the diagnostics also show repeated calls in `fn51`, `fn53`, and
`fn55`. This points to general effectful call/construction and field denotations
as compiler capabilities to investigate, but does not establish which would
improve score. DeltaBlue, Crypto, and NavierStokes have some native coverage
already (NavierStokes through OSR regions).

## Reproduction

```sh
for fixture in crypto deltablue navier-stokes richards; do
  QUENCH_JS_JIT_STATS=1 target/production/quench-node --jit \
    evidence/jit-jetstream3-2026-10-22/v8-v7-native-coverage/${fixture}.js \
    >target/current-v8-jit-coverage/${fixture}.stdout \
    2>target/current-v8-jit-coverage/${fixture}.stderr
done
```

Raw stdout and opt-in diagnostics are preserved in this directory. This is a
single-host coverage check, not a JetStream score/RSS qualification.
