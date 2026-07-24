# Harness Diagnostics Tools — SUPERSEDED

**Status:** superseded 2026-07-24 by `tasks/harness-roadmap.md` +
`docs/tools.md`. Kept for history only; do not follow the table below.

Correction: the env-vars this file claimed as "done"
(`TEST262_SHOW_SCRIPT`, `TEST262_DUMP_FAILURES`, `TEST262_RERUN_FAILURES`,
`TEST262_FIRST_N`) are **not read by the runner**
(`src/test262/runner/flags.rs`). The real failed-only rerun mechanism is
`TEST262_JSON=1` + `TEST262_FAILED_JSON=<path>`; script/stack dumping
lives on the `run-test` binary (`--show-script`, `--stack`). See
`docs/tools.md` for the verified env-var list.

Also note: this file predates the harness-fidelity rule
(`tasks/harness-roadmap.md` §Harness fidelity). The native `assert` /
`propertyHelper` reimplementations it describes are tracked as
removal debt, not as a pattern to extend.
