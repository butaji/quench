# Global root-binding cache: M4 Splay screen

Decision: **do not merge this performance change into the M4 trunk yet**. The
M4 Splay instruction screen is a small, consistent regression, while the cycle
result is too noisy to establish a gain or rule out a regression. The Linux
Callgrind result remains valid for Linux; it does not transfer to M4.

## Candidate and build

- M4 arm64 macOS, Mac16,10, Darwin 25.5.0, 10 logical CPUs, 16 GiB RAM.
- Trunk source baseline: `d599e1e9435a0ef8eb08608f8942273c887d1db8` (same runtime
  source as the post-merge baseline at `3d8020107`; later trunk commits are
  evidence-only).
- Candidate: the runtime changes from `ee14b1f4f` applied to that trunk. The
  obsolete `tasks/93.md` note was omitted, honoring the Linear migration.
- Build: `cargo build --profile production -p quench-node --bin quench-node`,
  with `CARGO_INCREMENTAL=0` and the checked-in M4 Cargo policy.
- Candidate binary SHA-256:
  `155d8eac3a6e9497109c7e7e0e84edd44656d17912e48364b4ea0ad8bbef9935`.
- Baseline binary SHA-256:
  `127b3fdc659ba5944bf03658d5f2e2f85b5971a4b91121e40a7e66dc00649543`.

## Correctness

- Candidate M4 runtime library: **543 passed, 0 failed**.
- The local Test262 checkout is missing `tests/test262/test/harness`, so the
  directive-prologue stage could not run here. The lane's Linux record reports
  Test262 stages 1–31, 47, 54, 60, 62, 64, 68 and 79–81 at baseline or better,
  plus Node-oracle probes for reassignment, accessor redefinition, deletion,
  configurable globals, many added globals and a frozen global object.
- The M4 fixed-work report completed with equal Splay output.

## M4 fixed-work result

Raw report: [`splay-fixed-work-contention-11.json`](splay-fixed-work-contention-11.json).
It uses the pinned V8-v7 `splay.js`, 300 `run()` calls per work sample, 11
alternating rounds, and the rebuilt fixed-work runner. The runner classified
**9/11 rounds clean**; two were excluded from clean-pair summaries. The baseline
and candidate binaries both passed every work and setup-only run.

The earlier [`splay-fixed-work-11.json`](splay-fixed-work-11.json) is retained
for provenance, but its runner predates the contention annotations. It is not
used for the decision; the detector-backed report supersedes it.

On the 9 clean paired rounds, marginal instructions per `run()` were:

- baseline: **33.079M** median;
- candidate: **33.199M** median;
- paired candidate delta: **+0.358%**, with every clean pair between **+0.331%
  and +0.487%**.

This exceeds the prior same-binary instruction A/A band of about **±0.14%**. It
does not support an M4 speed win. Marginal cycles were **5.693M → 5.750M** on
the clean-pair medians; the paired median delta was **+0.774%**, with a wide
range (**−8.683% to +1.970%**) that crosses zero. Treat cycles as inconclusive.

Median peak work RSS was **122.814 MB → 122.733 MB**, a **−0.067%** change within
RSS noise. Against the same M4 references, the fixed-work proxy distances are:

- speed: **1.22× → 1.23×** Node `--jitless` (Node marginal **4.679M cycles/run**);
- RSS: **2.09× → 2.09×** Bun no-JIT (Bun **58.753 MB**).

These are fixed-work diagnostics, not stock-harness qualification results. The
active Splay benchmark remains unwon on M4; its current stock-harness distances
are unchanged by this candidate.

## Follow-up

Keep the global-read cache off the M4 trunk until a new M4 hypothesis explains
the Linux/M4 divergence and a repeat screen demonstrates no M4 regression. The
current result does not justify rebasing the already-rejected compact-layout
candidate on top of this cache.
