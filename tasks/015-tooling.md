# Tooling and harness improvements

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
