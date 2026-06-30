# Task 49: Add local script to run TypeScript's own test runners

## Goal

Provide documented, reproducible commands for running TypeScript's test suite from the Quench repo so developers can generate reference baselines on demand.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: script the filtered runner commands, not the full 19k-case suite.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `scripts/run-typescript-tests.sh` (new)

## Steps

1. Create `scripts/run-typescript-tests.sh` that:
   - Enters `tests/typescript`.
   - Runs `npm ci` if `node_modules` is missing.
   - Runs `npm run build:tests`.
   - Runs `npx hereby runtests-parallel --no-lint --runner=compiler,conformance,project,transpile`.
2. Expose sharding via environment variables so large runs can be split across local workers if desired.

## Example script

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -d tests/typescript/node_modules ]]; then
  (cd tests/typescript && npm ci)
fi

(cd tests/typescript && npm run build:tests)
(cd tests/typescript && npx hereby runtests-parallel \
  --no-lint \
  --runner=compiler,conformance,project,transpile)
```

## Boundaries

- Only add local scripts; do not add GitHub Actions or other external CI configuration.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- `./scripts/run-typescript-tests.sh` runs without manual steps.
- The command is documented in `docs/typescript-tests.md` (Task 50).

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.

## Verification

```bash
./scripts/run-typescript-tests.sh
```
