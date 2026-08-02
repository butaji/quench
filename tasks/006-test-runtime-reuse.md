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

The explicit `--reuse-dir` mode reuses one rquickjs `Runtime` from the
`quench-node` crate while creating a
fresh context per script. Isolation and state-reset behavior are covered by
`tests/node-compat/stage-383/runtime-reuse.js` and its companion script.
Core process metadata is covered by
`tests/node-compat/stage-409/process-metadata.js`.
Unknown `process.binding()` modules are covered by
`tests/node-compat/stage-410/process-binding.js`.
The mode is documented in the CLI `--help` output; isolated execution remains
the default, and reuse is intended for compatible test groups that do not rely
on process-global native state.

The task is complete.
