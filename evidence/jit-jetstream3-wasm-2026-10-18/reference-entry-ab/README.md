# Same-artifact body-entry cache screen

This is an exploratory three-round JetStream 3.0 screen for caching the internal body entry pointer at same-artifact Wasm `call_indirect` sites. The generated hit path retained the epoch, environment, table-index, function-range, and non-null guards and removed the artifact-table lookup. The pointer was published only after a successful same-artifact call and final table-slot recheck.

The screen was valid in all three rounds and every engine produced equal output. The paired score estimate was effectively tied: feature on scored 11.29 versus 11.28 off (on/off ratio 1.00089; paired 95% bootstrap interval 0.98673–1.02527). Peak RSS increased by 16,384 bytes, from 236,552,192 to 236,568,576 bytes (ratio 1.000087; paired interval 1.000035–1.000104). The score interval includes no change, while the RSS increase is small but consistently positive. This candidate did not pass the speed and memory screen, so the implementation and its benchmark-only CLI mode were reverted; no 11-round confirmation was run.

The measured peer runtimes remained far ahead on this fixture: Bun 1.4.3 scored 210.31 with 131,514,368 bytes peak RSS, and Node v26.12.0 scored 197.49 with 146,571,264 bytes. This experiment did not produce a peer-runtime win.

The Quench arms used the same production executable (`94bc186e7af38cbf9f389adf601b3f25c0cf95ce3037a67b07c9d3c9e684db4c`) and identical `--jit` arguments; only `QUENCH_WASM_JIT_REFERENCE_ENTRY_CACHE=0|1` differed. Bun and Node versions and binary hashes are recorded in the raw report. The report is exploratory, with `source_dirty=true`, and is not qualification evidence.

Reproduction command:

```sh
target/production/quench-bench --fixtures tsf-wasm --suite jetstream3 --jit \
  --jit-wasm-reference-entry-ab --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/reference-entry-ab/screen-3-round.json
```

Validation: production build passed; `cargo check --locked -p quench-runtime --lib --tests` passed; the three focused `quench-bench` reference-entry tests passed. The focused runtime test binary was attempted twice, including once with reduced codegen units, but rustc was SIGKILLed while code-generating the full runtime test crate. The production differential screen itself exercised the feature-on generated path and validated all outputs.

Files:

- `screen-3-round.json` — raw engine samples, hashes, host data, and output validation.
- `screen-3-round-analysis.json` — paired score/RSS estimates and bootstrap intervals.
- `prototype.patch.gz` — compressed exact source diff used for this screen, including the runtime experiment and temporary benchmark mode. Decompress with `gzip -dc prototype.patch.gz`.
