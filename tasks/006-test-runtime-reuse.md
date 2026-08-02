# Add an isolated and reusable test execution mode

## Goal

Reduce harness startup overhead without weakening the existing per-test isolation mode.

## Scope

- Keep the current `run_source` behavior as the default.
- Add an explicit reusable mode for compatible test groups.
- Define how globals, module state, pending jobs, errors, and bootstrap state are reset between scripts.
- Extend CLI help and document when reuse is safe.

## Done when

- Existing isolated execution remains unchanged.
- Reusable execution has focused tests proving state does not leak between scripts.

## Status

The explicit `--reuse-dir` mode reuses one rquickjs runtime while creating a
fresh context per script. Isolation and state-reset behavior are covered by
`tests/node-compat/stage-383/runtime-reuse.js` and its companion script.
