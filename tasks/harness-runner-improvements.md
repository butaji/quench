# Harness Runner Improvements — Perf, Bugs, Overrides

Focused work on the test262 harness runner
(`crates/quench-runtime/src/test262/`), the `quench-test262` boundary, and the
`run-test` tool. No conformance rule changes. All changes are TDD'd.

## Status (2026-08-08)

### Done — performance

- **Thread-local harness-IR cache + in-thread runner**: `try_inject_harness`
  re-parsed ~25 harness JS files per test (twice for dual-mode tests) on a
  fresh thread. A per-thread IR cache (`Context::eval_program`, `HARNESS_IR_CACHE`)
  reuses one parse per file per thread; tests now run on the caller's thread
  (`run_single_test_in_thread`) so the cache carries across a worker's runs.
  Digest stage-0 serial: ~38s → ~3.7s (~12x).
- **Green gate `run_stage`** switched to the same in-thread path (stage 0
  ~8.2s → ~3.25s); dropped the unused `host` parameter from `run()`/`run_stage()`.
- **Digest defaults to fast in-process serial**; parallel only for isolated
  (subprocess) mode, which is race-free (in-process parallel can trip
  pre-existing promise/async races).
- Process-wide harness-file content cache (wipes a disk read per file per
  test); `Arc`-shared test list across digest workers; `DirEntry::file_type()`
  in collection (no extra stat per entry).

### Done — bugs

- **In-thread thread-local leak**: a stale `NEW_TARGET` (and other interpreter
  thread-locals) leaked between tests sharing one thread, breaking Symbol
  (and other stateful) stages. Added `interpreter::reset_interpreter_state()`
  at each top-level run. Symbol/generators stages now match isolated.
- Dead strict-mode save/restore in `host.rs` removed.
- **`run_isolated` killed the subprocess on timeout** (was `Command::output()`,
  which waited forever and left stale processes). Timeout tunable via
  `TEST262_TIMEOUT_SECS`.
- `quench-test262::discover_js_files` excluded `*_FIXTURE.js` (were treated as
  runnable tests).

### Done — overrides of test262 behavior (pinned, not tightened)

- `phase: parse` negatives accept any error (the engine does not yet produce
  those early errors; tightening regressed already-green stages). Pinned with a
  test + comment.
- Runtime-phase substring type match (expecting `Error` matches `TypeError`).
- Native overrides for `deepEqual` / `isConstructor` / `verifyProperty`
  (documented in `harness/mod.rs`).
- `quench-test262` has no dual-mode (sloppy+strict) run.

## Open

- [ ] Engine early-error implementation before `phase: parse` negatives can be
  tightened to require a genuine parse error.
- [ ] Exact error-type matching for runtime-phase negatives.
- [ ] Cooperative cancellation for the in-thread path (a hung test can't be
  killed in-process; use `TEST262_ISOLATED=1` for a time-bounded scan).
- [ ] Pre-existing engine unit-test failures (Symbol subclassing, class
  helpers, etc.) — engine work, outside the harness runner.

## Notes

- Perf is the SSOT via the stage run; run the stage to know where you stand.
- Commits on `main` (2026-08-08): harness content cache, dead-code cleanup,
  IR cache + in-thread runner, digest in-process default, thread-local leak
  fix, subprocess timeout/kill, `run_stage` in-thread path,
  `quench-test262` fixture discovery.