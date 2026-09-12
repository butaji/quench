# Mechanism probes

`quench-bench/deegen-curriculum/manifest.json` owns the existing probe inventory
and instrumentation expectations. Its reference to this manual remains valid.
Historical claims of mechanism coverage must be checked against actual emitted
events; proxy counters do not prove native execution, tier promotion or OSR.

```sh
node quench-bench/deegen-curriculum/deegen-verify.mjs --engine target-exec-trace/profiling/quench-node --oracle node --from 1 --to 5 --runs 1 --out target/curriculum-new-attempt.json
```

The output directory must exist; use a new filename. Instrumented timing is
diagnostic. The legacy runner can substitute physical footprint when RSS is
missing; do not label that fallback RSS. Missing or skipped observations cannot
satisfy native-coverage evidence for a claim. Use
[micros](../quench-bench/micros/README.md) for behavioral contrasts and [the
protocol](performance-lanes.md) for performance claims.
