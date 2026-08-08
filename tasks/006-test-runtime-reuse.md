# Add an isolated and reusable test execution mode

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

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
Built-in module lookup is covered by
`tests/node-compat/stage-411/process-builtin-module.js`.
The harness emits the final `process` exit event, covered by
`tests/node-compat/stage-429/process-exit-event.js`.
The mode is documented in the CLI `--help` output; isolated execution remains
the default, and reuse is intended for compatible test groups that do not rely
on process-global native state.

The task is complete.
The `async_hooks` surface now exposes execution resource/id accessors and
chainable hook enable/disable methods. This is covered by
`tests/node-compat/stage-493`.
Async resources are captured and restored across timers and HTTP request
handlers, covered by `tests/node-compat/stage-495` and the upstream
`test-async-hooks-execution-async-resource.js` test.
`AsyncResource` now supports IDs, scoped execution, static and instance
`bind()`, and argument validation. This is covered by
`tests/node-compat/stage-496`.
The minimal `child_process.spawn()` contract emits deterministic exit status
and signal values for harness fixture scripts, covered by
`tests/node-compat/stage-501`.
The minimal `net.createServer()` and child IPC send/backpressure contracts are
covered by `tests/node-compat/stage-502`.
Child processes launched with `-e` now report successful exit status and no
signal, covered by `tests/node-compat/stage-503`.
The minimal primary-process cluster fork and disconnect lifecycle is covered
by `tests/node-compat/stage-504`.
Cluster primary setup now clones settings and emits asynchronous `setup`
events, covered by `tests/node-compat/stage-505`.
The common child-process compile-cache assertion helper is covered by
`tests/node-compat/stage-506`.
