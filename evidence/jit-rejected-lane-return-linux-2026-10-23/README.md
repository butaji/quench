# Rejected JavaScript JIT functions: lane return screen

Source revision: `2dc8133450742eab74f06aaa5de88b2d5fcdfff7` (`v2-jit`). The
change makes a permanently rejected JS tier candidate take the ordinary
frameless lane call when diagnostics are disabled. Diagnostic execution keeps
the frame so rejected-entry counters remain available.

## Validation

- `cargo check --locked --profile iteration -p quench-runtime --lib --tests`
  passed with the requested iteration profile variables.
- A 300-call object read/write script produced identical output in Node,
  Quench interpreter mode and Quench `--jit` mode.
- The full runtime lib test compile was attempted with the requested
  `CARGO_PROFILE_ITERATION_OPT_LEVEL=0`,
  `CARGO_PROFILE_ITERATION_CODEGEN_UNITS=16`, and
  `CARGO_BUILD_JOBS=2`; Rustc approached the 16 GiB cgroup limit, so the run
  was stopped before the test binary was linked. This is not a green lib test
  result.

## Matched `typescript-lib` screens

Each mode used 11 rounds on the stock JetStream 3 fixture. All rounds validated
and outputs matched. Bun was 1.4.3; Node was v24.19.0 with JIT enabled for the
JIT screen and `--jitless` for the no-JIT screen. Linux x86_64, 4 logical CPUs,
400% CPU quota, 16 GiB cgroup memory limit. These reports cover one fixture and
do not qualify the full suite.

| Mode | Quench score / RSS | Bun score / RSS | Node score / RSS |
| --- | ---: | ---: | ---: |
| JIT | 0.15 / 186.5 MiB | 1.42 / 349.4 MiB | 1.88 / 345.2 MiB |
| No-JIT | 0.23 / 185.9 MiB | 0.48 / 188.4 MiB | 0.55 / 303.7 MiB |

Paired 95% score-ratio intervals for Quench are:

- JIT: 0.0859–0.1462 vs Bun and 0.0702–0.1212 vs Node.
- No-JIT: 0.4167–0.5952 vs Bun and 0.3696–0.4286 vs Node.

RSS ratios remain below 1 in both modes. JIT median score on this fixture is
about 0.65 times Quench's no-JIT median from a separate run, while median RSS
is almost unchanged. One JIT Quench sample took 281 seconds and scored 0.02;
it is included in the raw report. The change does not demonstrate a score
improvement, and both modes remain far below the user's ≥1.20× target.

## Artifacts and reproduction

- `typescript-lib-jit-report.json` and `typescript-lib-jit-analysis.json`
- `typescript-lib-nojit-report.json` and `typescript-lib-nojit-analysis.json`

Build the production binaries with:

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --locked --profile production -p quench-node -p quench-bench
```

The runs used `quench-bench --fixtures typescript-lib --suite jetstream3
--runs 11 --timeout-ms 600000`, with `--jit` for the JIT report and a
checkpoint for each mode. The runner's report includes full executable hashes,
round order, raw samples, JetStream corpus revision and output validation.
