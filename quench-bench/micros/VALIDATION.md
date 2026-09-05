# Harness and corpus validation

Validated on 2026-09-04 on ARM64 macOS (Apple M4), with Node v26.7.0 and Bun
1.3.14. The candidate was Node for these harness checks; this is not a Quench
performance or compatibility qualification.

- All 124 new variants passed development-input semantic checks.
- The final frozen-edition run passed all 844 scenarios: 744 reserved scenarios
  (124 variants × 3 sizes × 2 seeds, with wrapped source) and 100 legacy cases.
- The final run had zero semantic failures and no source/binary identity warnings.
- All 20 harness tests passed, covering statistics, lifecycle verdicts,
  architectural neutrality, missing/truncated instrumentation, graph validation,
  semantic effects, timeouts, async completion, and legacy strict-mode behavior.
- ESLint passed for new JavaScript code; `git diff --check` passed.
- Development timing/fixed-work RSS and a three-replicate lifecycle smoke run
  exercised actual process accounting and external RSS sampling. These were
  operational checks during development, not qualification evidence.

Reproduce the final corpus check:

```sh
node quench-bench/micros/run.mjs smoke --engine node --reserved --size all \
  --include-legacy --out target/micros/new-validation-attempt.json
node --test quench-bench/micros/tests/harness.test.mjs
```

The local complete evidence is in
`target/micros/final-corpus-validation.json` and its adjacent Markdown report.
Artifacts are intentionally not tracked. The frozen edition digest is
`aaf42feca9cf6ee0d1ddc15dbdbaf193707b52cd8f1837fc0941b6e2f01f7571`.

No Quench runtime changes or optimizations were performed for this delivery.
The optional adapter consumes existing trace snapshots. Chronological events
remain explicitly unavailable through that interface. Full performance
qualification was not run.
