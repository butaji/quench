# Tooling and harness improvements

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target. Tooling must report
explicit manifest statuses and application-gate regressions.
Use `docs/authoritative-test-sources.md` as the source map for Node, LLRT,
Deno, WPT, and Test262 integration.

## Goal

Make the per-slice feedback loop fast, deterministic, and self-explanatory.
Each new slice is one stage + one polyfill change + one commit. The
surrounding tooling should:

- Run the focused stage in < 2 s (per stage), the full focused suite in
  < 5 min.
- Run an up-stream cluster via `tools/measure-node-tests.sh` and emit a
  per-fixture diff vs the previous run.
- Format with Prettier (default config) and check `git diff --check`
  automatically.
- Emit a one-line "what changed and what to do next" summary.

## Existing tools (keep, audit)

- `tools/run-node-tests.sh` — runs a single up-stream fixture or directory.
  It terminates runs after 30 seconds by default; override with
  `QUENCH_NODE_TEST_TIMEOUT_SECONDS`.
- `tools/measure-node-tests.sh` — measures pass rate over a directory.
  Each fixture has a 10-second timeout by default; override it with
  `QUENCH_NODE_TEST_TIMEOUT_SECONDS` so hanging upstream leak tests are
  reported as failures instead of blocking the corpus audit.
- `tools/check-focused-stages.sh` — runs all focused stages.
- `tools/check-focused-stages-parallel.sh` — runs focused stages in parallel
  with the same per-stage timeout.
- `tools/compat-coverage.sh` — emits counts (focused stages, up-stream
  fixtures, parallel fixtures).
- `tools/lint-stage-numbers.sh` — rejects gaps in the focused stage sequence.

## Backlog

### High value

1. **Prettier integration.** Add a `.prettierrc.json` (defaults) and a
   pre-commit hook that runs `npx prettier --check` over the changed
   JS files. Document the install step (the repo currently has no
   `package.json`).
2. **`tools/diff-cluster.sh <prefix>`** — git-aware:
   - Run `tools/measure-node-tests.sh tests/node/test/parallel <prefix>`
     on the current commit, then on the previous commit, and emit a
     per-fixture diff.
3. **`tools/coverage-by-prefix.sh`** — emit a `prefix pass / total`
   table from `tools/measure-node-tests.sh` over every prefix in
   `tests/node/test/parallel`. Update `tasks/013-upstream-fixtures.md`
   from this output.
4. **`tools/lint-host.sh`** — rustfmt + clippy for
   `crates/quench-node/src/`. Pin to the project's `.clippy.toml`.
5. **`tools/slice-template.sh <N> <name>`** — generates
   `tests/node-compat/stage-N/<name>.js` from a template that pulls in
   `assert`, `common`, and the exit-event assertion.
6. **`tools/host-callback-contract.md`** — template for documenting a
   new `__quench_*` callback: name, args, return type, error mode,
   side effects. Linked from each new entry in `tasks/014-host-surface.md`.

### Medium value

7. **Stage numbering**: ensure `tests/node-compat/stage-N` directories
   are contiguous and gap-free. Add a `tools/lint-stage-numbers.sh`
   check.
8. **Skip markers**: a fixture can opt out via
   `// QUENCH-SKIP: <reason>` in the focused stage. The runner honours
   the marker and counts it as a "skip" not a "fail".
9. **Per-slice "diff footprint"** in the commit message: a
   `tools/commit-msg.sh` that appends
   `git diff --stat HEAD~1 -- crates/ tests/` to the message.

### Low value

10. **Coverage HTML report** generated from
    `tools/coverage-by-prefix.sh`.

## Slicing rules

- One tool per slice.
- Tools land as separate `tools/*` files; the runner script is updated
  in lockstep.
- The host-callback contract template is a single Markdown file in
  `tools/`.

## Done when

- Every commit is auto-formatted and `git diff --check`-clean.
- The focused suite runs in < 5 min locally.
- `tools/diff-cluster.sh <prefix>` returns a useful diff in < 30 s per
  cluster.

## Enforced source limits

The repository now enforces these limits for every non-vendored `.js` and
`.rs` source file:

- 500 physical lines per file (`tools/lint-size.sh`)
- 40 lines per function (`max-lines-per-function` in ESLint and
  `clippy::too_many_lines`)
- complexity no higher than 10 (`complexity` in ESLint and
  `clippy::cognitive_complexity`)

Run `tools/lint-all.sh` locally (or the language-specific scripts). GitHub CI
is intentionally absent; local verification is the repository gate. Existing
oversized bootstrap and host files are reported as failures and must be
decomposed in subsequent slices.

## Status

In progress. The slice template now seeds an exit-event assertion, and
`tools/lint-host.sh` now provides the documented host-only Rust gate. Prettier is
now pinned in
`package.json` with a local staged-file hook installed by
`tools/install-hooks.sh`, and the host callback contract template is available
as `tools/host-callback-contract.md`.
Repository-wide
source-limit enforcement is now installed. The bootstrap diagnostics added for
stage 514 exposed and fixed an initialization-order bug in the stream polyfill.
Focused runners now use a forced kill-after grace period so timed-out child
processes cannot accumulate as stale runners.
Stage 516 also confirmed that one shared callback adapter is sufficient for the
zlib convenience methods while preserving synchronous validation and async
error delivery.
Stage 518 added zlib unzip autodetection and confirmed the existing compressed
byte primitives can support both gzip and deflate without another host hook.

The local Rust test profile is now configured in `.config/nextest.toml`.
`tools/check-all-tests.sh` uses cargo-nextest when installed, followed by the
parallel Node harness; GitHub CI remains intentionally absent.
The bootstrap decomposition now also extracts util, querystring, URL, crypto,
streams, OS, and performance implementations into readable files under the
500-line limit; runtime concatenation preserves initialization order and the
existing focused stages remain green.
The core, text/assert, and path/common sections are now extracted as well;
the remaining monolith is concentrated in the Buffer and fs implementations.
`tools/diff-cluster.sh` now compares fixture outcomes between `HEAD^` and the
working commit, making per-slice regressions visible without manual log
comparison.
`tools/coverage-by-prefix.sh` now emits pass/total percentages grouped by the
leading fixture prefix for corpus-level progress measurement.
The unused ESLint suppression in the dgram membership polyfill was removed, so
the repository lint gate is warning-free for owned source.
The Node harness now prints the underlying error message instead of only the
QuickJS error category when an upstream fixture fails.

The parallel differential runner now builds `quench-node` once before
fan-out, passes that verified binary explicitly to every worker, and refuses
to publish a report when any worker fails. Worker reports are discovered from
a deterministic sorted directory rather than a concurrently appended list;
the persisted report also records the worker count and timeout. This prevents
partial corpus runs from being mistaken for complete evidence and removes
Cargo lock contention from parallel triage.

Focused failures now retain one log per stage under
`target/compat/focused-logs/latest` (override with
`QUENCH_FOCUSED_LOG_DIR`). The serial gate remains authoritative for fixtures
that share repository-relative paths, while failed-stage diagnostics are
available without reproducing the run.

`tools/audit-platform-coverage.sh` audits platform ownership claims without
changing runtime behavior. It checks that every platform prefix and fixture
pattern names real fixtures, stream ownership has no overlapping prefix,
differential-report prefixes agree with the runner, and every exemption has a
quench-side non-match. It first requires a fresh schema-2 report, so stale or
partial differential data cannot justify a platform classification. Set
`QUENCH_COMPAT_ALLOW_STALE=1` only for static ownership checks while a report is
being regenerated; that does not make the report authoritative.

`tools/compat-decision-report.sh` converts a differential report into a small
JSON decision snapshot. It ranks owned and unclassified signature clusters by
category and observed fixture cost, compares an optional previous report for
resolved/regressed fixtures, checks report freshness, and lists missing data
that blocks cache or persistent-worker decisions. It is intentionally
read-only and remains useful while a report is stale:

```sh
tools/compat-decision-report.sh \
  target/compat/differential-current-post-crypto.json \
  target/compat/differential-current.json \
  target/compat/focused-stage-metrics.jsonl \
  target/compat/compat-decision.json
```

The current decision snapshot explicitly reports four evidence gaps that limit
throughput decisions: no previous report for trend/regression comparison, no
retry/flake history, no structured capability-probe frames, and no worker-level
startup/cache timing. These are measurement gaps, not compatibility claims;
the current canonical parallel report is nevertheless fresh and auditable.

The focused-stage cleanup also removes the known root-level artifacts emitted
by stages 2021 and 2023. This keeps generated files from being misclassified
as runtime failures or unclassified focused conflicts.

The cleanup change was verified with
`QUENCH_FOCUSED_STAGE_FROM=1256 QUENCH_FOCUSED_STAGE_TO=1256
tools/check-focused-stages.sh`: the stage passed without retries or runtime
failures.

The cleanup manifest also covers the `access-mode` file emitted by the fs
access focused stage, preventing that application fixture from becoming an
unclassified gate conflict.

The runner also performs cleanup after the final stage, not only before each
stage. The grouped 2058–2060 gate passes 3/3 with no leftover `access-mode`
file.

Stage discovery now filters the parallel runner to numeric `stage-*`
directories, preventing fixture-package directories from reaching integer
comparisons. With six workers and a 10-second per-stage timeout, stages 1–500
completed with 500/500 passing and zero failed stages.
The same sweep for stages 501–1000 also completed with 500/500 passing and
zero failed stages.
Stages 1001–1500 completed with 485/485 discovered stages passing and zero
failed stages.
Stages 1501–2000 completed with 428/428 discovered stages passing and zero
failed stages.
Stages 2001–2554 completed with 474/474 discovered stages passing and zero
failed stages using the normal 30-second timeout; stage 2223 is intentionally
slow and is a false timeout at the tighter 10-second diagnostic limit.

A fresh full-range parallel verification on commit `e6900c102` completed in 92
seconds: 2,388/2,388 focused stages passed, with zero failed stages and zero
covered policy failures using six workers.

The stage-2469 owned regression fixture was refactored into predicate and
abort-validation helpers. It now passes its stage and ESLint with zero
warnings, removing one function-size violation without changing assertions.

The zlib CRC32 validation was extracted from the checksum loop, reducing its
complexity violation while preserving input and seed validation; zlib stage
2031 remains green.

Zlib stream flush and parameter validation were extracted into helpers. The
owned zlib-streams lint errors dropped from five to two, and stages 522, 1232,
1778, 1780, 1783, 1784, and 1785 all pass.

The remaining zlib-streams factory/wrapper functions were split into stream
factory helpers. The module now passes ESLint with zero warnings and all
targeted zlib stages remain green.

The repository ESLint configuration now uses a dedicated global ignore entry
for the vendored `tests/node` submodule. The owned `tools/compat-decision-report.cjs`
classification function was split to satisfy the complexity limit; that file
passes ESLint with zero warnings. The full owned JavaScript tree still has
remaining file/function/complexity violations and is not yet clean.

Buffer float-offset range-error construction was extracted into a helper; the
buffer-validation module now passes ESLint with zero warnings, and stages 1, 2,
and 1005 remain green.

The REPL startup factory now passes ESLint with zero warnings after extracting
its finalization step; stages 533, 1153, and 1949 all pass.
