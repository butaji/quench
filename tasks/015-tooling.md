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

- `tools/run-node-tests.sh` — runs a single up-stream fixture.
- `tools/measure-node-tests.sh` — measures pass rate over a directory.
- `tools/check-focused-stages.sh` — runs all focused stages.
- `tools/compat-coverage.sh` — emits counts (focused stages, up-stream
  fixtures, parallel fixtures).

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
11. **CI workflow** at `.github/workflows/ci.yml` that runs
    `check-focused-stages.sh` and `tools/measure-node-tests.sh` on every
    push.

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

## Status

In progress. Items 1, 5, and 6 are the immediate next slices.
