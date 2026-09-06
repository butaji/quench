# Legacy mechanism curriculum

The files in `quench-bench/deegen-curriculum/` are mechanism-oriented probes.
Its `manifest.json` owns case inventory and instrumentation expectations.
This manual does not freeze internal counter values or claim current coverage.

For architecture-neutral measurements and qualification use
[the micros manual](../quench-bench/micros/README.md). Keep legacy experiments
deferred alongside micros until the [infrastructure gate](stencil-jit-implementation-spec.md)
passes; runtime correctness tests remain available during implementation.

## Use

From the repository root with an existing trace-enabled executable:

```sh
node quench-bench/deegen-curriculum/deegen-verify.mjs --engine target-exec-trace/profiling/quench-node --oracle node --from 1 --to 5 --runs 1 --out target/curriculum-new-attempt.json
```

The output directory must already exist; choose a new report filename.
The runner compares output, runs declared instrumentation checks and also
produces timing/memory ratios. Ratios from this trace-enabled binary are
diagnostic only, not controlled performance evidence.

## Limits

- Missing, skipped or proxy counters do not prove a native mechanism executed.
  Verify the actual trace schema before interpreting manifest expectations.
- Output correctness, counter checks and performance ceilings are different
  findings. No historic pass count or timeout list is a current result.
- The legacy runner can substitute physical footprint when RSS is absent;
  do not present that fallback as peak RSS. Use fixed-work micros accounting
  for memory comparisons.
- This curriculum does not certify the stencil gate or frozen micros edition.
  Its prior design and findings are available in Git history.
