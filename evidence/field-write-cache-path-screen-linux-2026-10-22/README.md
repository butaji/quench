# Shared lane field-cache write path screen

Date: 2026-10-22

## Question

Does routing non-private lane field writes through `Vm::set_field_cache_hit` instead of `Vm::cached_field_write_hit` improve no-JIT JetStream performance? The candidate can use the general field-cache path when method-cache references are active; it preserves the same cached-write semantic checks and falls back at the current instruction on a miss.

## Builds and method

- Base revision: `3ec81f100984739d6520e69f9322a73c948b50c9`.
- Baseline binary SHA-256: `9301d563d229eb79c3c75363e9c2115d3f0504ee06456de3b21fec058fd665a6`.
- Candidate binary SHA-256: `e3f38be7c6aec0cd3148b23257189552cbbd65ce6deddca86efe6e824854225e`.
- Production profile, no JIT, JetStream 3.0; Bun 1.4.2 with `useJIT=false`, Node v24.19.0 with `--jitless`.
- Three-round Acorn/Babylon reports were run in separate batches. Their reference-engine scores shifted substantially, so they are retained as a rough screen only, not treated as a matched performance comparison.
- Babylon was rerun in alternating one-round batches in B-C-C-B order. Each runner report validates the fixture output. Raw runner JSON is included here.

## Results

The alternating Babylon samples are effectively tied on score. Baseline Quench scores were 1.75 and 1.88 (median 1.815); candidate scores were 1.83 and 1.84 (median 1.835), about +1.1% in this two-sample screen. Median Quench RSS was 50,233,344 bytes for baseline and 48,400,384 bytes for candidate (about 3.6% lower). This is not enough evidence of a score gain and is far below the 1.20x goal.

The un-interleaved three-round Acorn/Babylon screen reported lower candidate scores, but Bun and Node scores also moved substantially between batches. Treat that result as host drift/noise, not a reliable regression estimate.

The candidate also passed `cargo check --locked --profile iteration -p quench-runtime --tests`, production build, direct output-parity cases, and JetStream fixture validation. The runtime unit-test binary was not run: compilation approached the 16 GiB cgroup memory limit and was interrupted before execution. Because no convincing score improvement was measured and the unit tests could not be completed, the source change was discarded; only the evidence remains.

## Artifacts

- `baseline-screen.json`, `candidate-screen.json`: separate three-round Acorn/Babylon screens.
- `babylon-b1.json`, `babylon-c1.json`, `babylon-c2.json`, `babylon-b2.json`: alternating one-round reports.

Diagnostic lane-exit reductions from instrumented runs are not a performance result and are not included in score qualification.
