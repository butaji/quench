# Resumable fixed-work guard

`quench-bench/src/bin/task61-fixed-work-guard.rs` makes the all-eight fixed-work guard resumable by fixture. It is an analysis/orchestration tool; the fixed-work measurement runner and pinned Quench executables remain the sources of raw data.

Start records a validated all-eight report and writes a checkpoint plus a derived decision file without launching benchmarks:

```sh
cargo run -p quench-bench --bin task61-fixed-work-guard -- \
  start --initial-report REPORT.json --checkpoint STATE.json \
  --change-kind semantic-neutral-metadata-or-dispatch
```

Use `--change-kind layout` for representation or code-layout changes. In a quiet window, `resume --checkpoint STATE.json` runs only fixtures still inconclusive. Each batch invokes the exact runner and Quench binaries recorded in the initial report. The tool checkpoints before and after each batch, preserves each raw report, and checks report, engine, runner, source, corpus and suite-input identities before combining rounds. `status --checkpoint STATE.json` recomputes the decision without running benchmarks. The merged decision is written to `STATE.decision.json`; top-up reports are stored beside the checkpoint in `STATE-topups/`.

The named guard policy is seven clean pairs per fixture, followed by at most two top-up batches of seven rounds each. Cycle medians and 95% paired bootstrap intervals use clean pairs only. Semantic-neutral metadata/dispatch changes may classify a fixture as `instructions_only` when cycles remain contaminated and at least eleven valid pairs show a median instruction delta no higher than the measured +0.14% A/A band. Layout changes cannot use this fallback. The RSS guard is a +0.5% median maximum-RSS budget, computed from valid paired samples; it remains an independent gate.

## Current decisions

The direct-eval-only captured-`with` metadata report has seven fixtures with at least seven clean pairs. RayTrace has zero clean cycle pairs, but its instructions-only fallback passes: −1.087% median over eleven valid pairs. The candidate is still rejected by the RSS guard: NavierStokes +0.568% and RegExp +0.765% median maximum RSS. Its resumable record is [the checkpoint](../task61-splay-call-path-attribution-2026-10-10/captured-with-resumable-guard.json) and [the merged decision](../task61-splay-call-path-attribution-2026-10-10/captured-with-resumable-guard.decision.json). No RayTrace top-up is needed unless the RSS decision is deliberately re-opened.
The source is the [all-eight fixed-work report](../task61-splay-call-path-attribution-2026-10-10/captured-with-all-eight-rerun-2026-10-10.json).

Cold-payload boxing is a layout change. DeltaBlue and EarleyBoyer have only three clean pairs each, so they remain queued for clean-cycle top-ups. The existing clean data already shows RegExp +3.189% cycles (95% interval +1.041% to +3.888%), Richards +2.382% (+1.763% to +2.583%), and NavierStokes +1.140% median RSS. Its [checkpoint](../task61-splay-cold-cell-payloads-2026-10-09/resumable-guard-2026-10-10.json) and [merged decision](../task61-splay-cold-cell-payloads-2026-10-09/resumable-guard-2026-10-10.decision.json) preserve those observations while leaving only the under-sampled layout fixtures for top-up.
The source is the [all-eight fixed-work report](../task61-splay-cold-cell-payloads-2026-10-09/all-eight-detector-rerun-2026-10-10.json).
