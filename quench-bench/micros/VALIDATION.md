# Harness and corpus validation

Validate the runner independently before interpreting Quench results:

```sh
node --test quench-bench/micros/tests/harness.test.mjs
node quench-bench/micros/run.mjs smoke --engine node --reserved --size all --include-legacy --out target/micros/new-validation-attempt.json
```

Use a new output path. Record the inventory and edition identity from the current
runner rather than copying historical scenario totals. Repeat semantic checks
with the candidate Quench binary before timing. A Node-as-candidate harness pass
does not establish Quench compatibility or performance.

See the [manual](README.md) for supported diagnostics and frozen qualification,
and the [performance protocol](../../docs/performance-lanes.md) for VM acceptance.
