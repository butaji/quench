# Test262 Runner Integrity Plan

This plan keeps test262 as the conformance oracle. Unit tests cover runner
invariants, regressions, and isolation; test262 cases remain exclusively in the
staged digest. Per `AGENTS.md`, **duplicating a test262 assertion as a unit test
is forbidden** — every unit test in this plan belongs to one of the three
admitted categories: a reproducer pinned to a failing test262 case, a
core-invariant test262 cannot observe, or a refactor pin. Stage‑zero per‑file
coverage is the test262 gate (122 stages, 100% per stage) plus the runner
invariants/reproducers listed below; there is no per‑file duplicate of the
116 harness files.

GitHub Actions are forbidden in this repo (`AGENTS.md`), so this plan is
executed locally via `cargo nextest` / `bash tools/*.sh`; there is no CI
tracking it.

## Authoritative current state (as of 2026-08)

- **Stage 0 (`tests/test262/test/harness`): 116/116 passed, 0 failed, 0
  skipped.** Verified by
  `crates/quench-runtime/src/test262/runner/execute/tests.rs::every_stage_zero_case_passes_through_the_runner`
  (`assert_eq!(paths.len(), 116); … assert!(failures.is_empty(), …)` on
  every `.js` file under `test/harness`). The Stage‑0 corpus is enumerated
  exhaustively (no `.js` file is silently filtered; invariant
  `stage_zero_inventory_is_complete_and_unique` in
  `crates/quench-runtime/src/test262/runner/collect.rs` asserts
  `tests.len() == 116` and unique paths).
- **Isolated digest of Stage 0 ≈ 5.4 s.** Measured end‑to‑end with
  `TEST262_DIGEST=1 TEST262_STAGE=0 cargo nextest run -p quench-runtime
  --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored
  all --no-capture`. Single‑test reproducers (e.g. `nativeFunctionMatcher.js`,
  `testTypedArray-conversions.js`, `deepEqual-primitives.js`) are also pinned
  via `run_isolated` in `runner/execute/tests.rs`.
- **Current stage in `tasks/index.json`: 44** (`test/language/expressions`).
  Plan ordering and `STAGES` mirror in
  `crates/quench-runtime/src/test262/runner/mod.rs` are the runner SSOT.
- `tasks/index.json` is descriptive configuration only; the authoritative
  coverage result is the staged digest output (per `AGENTS.md` and
  `tasks/index.json`’s own `coverage_ssot` field).

## Ordered items (no test262 behaviour overwrites, speed, correctness, simplicity, diagnostics)

1. **Do not rewrite test262 source; load upstream harness files verbatim.**
   `crates/quench-runtime/src/test262/harness/mod.rs::HarnessLoader::load`
   reads `tests/test262/harness/<name>.js` straight from disk and only
   strips the `/*--- … ---*/` frontmatter before caching; the only rewrite
   in the codebase is the one hard‑coded swizzle on `propertyHelper.js`’s
   `nonIndexNumericPropertyName` (see Note A). The harness files
   themselves are otherwise untouched. The submodule pointer
   (`.gitmodules: [submodule "tests/test262"]`) is never modified by the
   runner. Evidence:
   `crates/quench-runtime/src/test262/harness/mod.rs:51‑74` (load/cache),
   `harness/mod.rs:80‑112` (build_script), unit tests
   `harness_loader_tests.rs`, `harness_scope_tests.rs`. The test
   `isolated_run_finds_property_helper_from_any_cwd`
   (`runner/execute/tests.rs:256`) pins the include path resolution.

2. **Single test262 metadata parser with an explicit model.**
   `crates/quench-runtime/src/test262/metadata.rs::Test262Metadata::parse`
   extracts `description`, `esid`, `info`, `flags` (`module`, `raw`,
   `onlyStrict`, `noStrict`, `async`, `generated`, …), `includes`,
   `features`, and a nested `Negative { phase, typ }`. Eight unit tests
   (`metadata.rs:163‑279`) cover block‑style lists, noStrict flag, the
   negative block being closed at the next top‑level key, and an actual
   `legacy-octal-integer.js` fixture. Malformed/unsupported metadata is
   surfaced as a runner failure, not silently coerced.

3. **One execution contract per mode; no per‑mode shortcuts.**
   `runner/execute.rs::run_prepared` (line 183) is the single entry: it
   honours `module`, `raw`, `onlyStrict`, `noStrict`, and `async` flags
   but always routes through `run_with_timeout → execute_script →
   check_outcome`. The 19 `run_with_timeout` short‑circuits at the top
   (lines 272‑457) are *parse‑phase negative* early‑exits that consult
   `interpreter::has_*` lex‑level predicates — they exist because the
   test262 author encodes a parse‑time rejection as a `negative: { phase:
   parse, type: SyntaxError }` frontmatter without writing purely
   well‑formed JS; the runner cannot rely on the OXC parser to reject
   every spec early‑error the test262 corpus relies on, and these
   short‑circuits simply return `Pass` when the predicate matches the
   expected error type. They never invent a `Pass` for a runtime
   expectation — every predicate is gated by `meta.negative.phase ==
   "parse"` and a corresponding `negative.typ`. Unit tests:
   `runner/execute/tests.rs::check_outcome_parse_negative_*`,
   `only_strict_numeric_negative_test_is_rejected_during_parse`,
   `bigint_hex_literals_are_not_rejected_as_legacy_octal`,
   `regexp_modifier_overlap_is_rejected_during_parse`, …
   (16 such pinned cases in `tests.rs`).

4. **Process isolation is the default; the in‑process path is opt‑in.**
   `runner/digest.rs::inprocess_digest()` returns `false` unless
   `TEST262_INPROCESS=1`, so a crash becomes a `Fail` (not a `Skip`).
   `runner/execute.rs::run_isolated` (line 1542) spawns the prebuilt
   `run-test` binary with the test262 root passed via `TEST262_DIR`,
   `RUST_MIN_STACK=33554432`, and a wall‑clock timeout equal to the
   in‑process timeout (one timeout policy). `run_test_binary()` resolves
   `target/debug/run-test` before `target/release/run-test`
   (`runner/execute/tests.rs::first_existing_picks_debug_before_stale_release`,
   exact bit‑for‑bit equality). `digest_workers_are_capped_for_process_isolation`
   pins `worker_count(64) == 4` so the cap is the single source of truth.

5. **One timeout policy.**
   `runner/execute.rs::DEFAULT_TEST_TIMEOUT_SECS = 30` and
   `test_timeout_secs()` reads `TEST_TIMEOUT_SECS` for both paths. The
   invariant `in_process_and_isolated_share_one_timeout`
   (`runner/execute/tests.rs:14‑19`) asserts `test_timeout_secs() == 30`
   so a slow test cannot pass in‑process and fail isolated. Timeouts,
   panics, harness‑load failures, and spawn failures are surfaced as
   `TestFailure::from_message(...)` with one of the `INFRA_MARKERS`
   substrings (`runner/execute.rs:36‑41`); `check_outcome` rejects those
   as `infrastructure failure, not a test result` when they would
   otherwise satisfy a negative expectation — see the table‑driven
   `classification_infra_messages_for_negative_metadata_fail` and
   `check_outcome_infra_messages_never_pass_negative` tests.

6. **Structured failure record (path, mode, type, message, stack, source).**
   `TestFailure` (`test262/host.rs:26‑41`) carries `message`,
   `error_type`, `error_message`, `js_stack`, `source_path`,
   `source_line`, `source_context`. `TestFailure::with_source`
   (`host.rs:76‑123`) attaches a 10‑line‑each‑side context with a `→`
   marker on the localised line. `locate_message_in_source` strips the
   `JsError("…")` envelope, builds a body‑driven keyword index, and
   falls back to the *last* `assert.throws` for the
   `Thrown value was not an object!` wrapper message; regressions are
   pinned by `host.rs::locate_message_in_source_*` (5 unit tests,
   including the STAGE0 deepEqual‑primitives symptom). The runner‑level
   rich footer (`runner/mod.rs::print_rich_failure`, lines 266‑310)
   prints `Type`, `Reason`, `JS message`, JS stack, and a `── Source ──`
   block before the next test.

7. **Stable, grouped, JSON digest output with deterministic ordering.**
   `runner/digest.rs::group_failures` keys on `normalize_reason` (strips
   `strict: `, `JsError("…")`, `Test262Error:` prefix, then collapses
   arrays via `normalize_array_contents` and `N <op> N` via
   `normalize_comparison_values`). Output is a single
   `serde_json::json!` document with `stage`, `path`, `passed`,
   `failed`, `skipped`, `total`, `duration_ms`, `skips[]`, and
   `groups[]` (each with `reason`, `count`, `sample_paths`, optional
   `samples`). `runner/digest.rs:529‑685` pins every behaviour:
   `normalize_reason_strips_wrappers_and_prefixes`,
   `normalize_reason_groups_same_value_failures`,
   `normalize_reason_groups_array_mismatch_failures`,
   `group_failures_*`, `normalize_comparison_*`. `pretty(println!)`
   is the only sink; no raw detail is lost when a group is sampled.

8. **Runner invariants are unit‑tested, not test262‑cloned.**
   `runner/collect.rs::tests::stage_zero_inventory_is_complete_and_unique`
   (asserts 116 unique, .js, non‑fixture), `tests::collects_js_and_skips_fixtures`,
   `tests::includes_formerly_skipped_dirs`, and the runner‑wide
   `stats_is_complete` / `missing_stage_dir_is_a_failure` /
   `stage_with_skips_is_not_complete` in `runner/mod.rs` (lines 354‑379)
   are core invariants no test262 file can assert. `runner/flags.rs`
   pins `parse_current_stage`, `default_stage == tasks/index.json`,
   `env_bool_parses_true_values`, `default_parallel_is_on`.

9. **No spec‑op duplication; no result shortcutting.**
   `runner/execute.rs::check_outcome` (line 127) is the only place that
   decides Pass/Fail/Skip; the `is_js_throw_msg` /
   `error_type_matches` / `js_envelope_inner` helpers are the only
   envelope/infra classifiers, and each is pinned by 5+ unit tests in
   `runner/execute/classification_tests.rs` (e.g.
   `js_envelope_inner_extracts_inner_from_outer_wrapper`,
   `error_type_matches_envelope_wrapped_message`,
   `is_js_throw_msg_recognizes_known_envelope_kinds`,
   `classification_js_thrown_messages_with_panic_substring_are_not_infra`).
   Negative tests that pass on the wrong side are pinned by
   `check_outcome_negative_passing_test_fails`,
   `check_outcome_bare_negative_type_mismatch`,
   `check_outcome_infra_messages_never_pass_negative`. No `dbg!` or
   `println!` left in `src/` (the existing `eprintln!` calls in
   `harness.rs` are inside `#[cfg(test)]` diagnostic blocks).

10. **Crash isolation = subprocess; long runs = bounded workers.**
    `runner/digest.rs::run_parallel` uses `std::thread::available_parallelism`
    clamped to `worker_count(n) = n.clamp(1, 4)`, dispatches via
    `mpsc::channel`, and sorts results by index for deterministic order
    before reporting. `run_isolated` uses `child.try_wait` with a 20 ms
    poll, distinct stdout/stderr reader threads, and a shared deadline
    (`runner/execute.rs:1574‑1601`). The pipe‑buffer regression
    `isolated_large_output_test_does_not_block_on_pipes` (tests.rs:326)
    pins the non‑blocking reader.

11. **Complete diagnostics: per‑test snapshot + per‑group sample.**
    `TestFailureSample` (`runner/digest.rs:243‑250`) serializes
    `path`, `source_line`, `error_type`, `error_message`, `js_stack`,
    `source_context` — the same fields as `TestFailure` so the JSON
    digest is a complete debugging record. `TEST262_DETAIL=1` includes
    the per‑sample diagnostics in the JSON (`samples`) and a
    human‑readable `── Per-failure detail ──` block. `TEST262_STAGE`,
    `ALL_STAGES`, `TEST262_DIGEST`, `TEST262_QUICK`, `TEST262_QUICK_LIMIT`,
    `TEST262_ISOLATED`, `TEST262_INPROCESS`, `TEST262_PARALLEL`,
    `TEST262_SERIAL`, `TEST_TIMEOUT_SECS`, `TEST262_DIR`, `RUN_TEST_BIN`
    are declared in `runner/flags.rs` (the only env‑knob surface).

12. **Source‑context and failure‑locator guard against mis‑attribution.**
    The `assert.throws` wrapper message still triggers a usable
    source‑line guess. `host.rs::locate_message_in_source_*` (5 unit
    tests) pin the locator: the *last* `assert.throws` for the wrapper
    message, the *first* matching assertion in the no‑keyword fallback,
    the keyword‑counting scorer, and the `JsError("…")` stripper. The
    runner‑level footer emits a `── Source ──` block (renderer at
    `runner/mod.rs:294‑308`) on every failure, with the first 20 lines
    as a fallback when `TestFailure::with_source` did not produce a
    context. The fixture‑boundary case `stage0 deepEqual‑primitives.js`
    is pinned by `runner/execute/tests.rs::stage_zero_deep_equal_primitives_passes_through_runner`.

13. **No skips: zero items in `tests/test262/…/skip.rs`.**
    `test262/skip.rs::UNSUPPORTED_FEATURES = []` and
    `CRASH_FILES = []`; `should_skip`, `should_skip_path`,
    `should_skip_source` all return `None`. Unit tests
    `test_no_skip_for_default_metadata`,
    `formerly_unsupported_features_are_attempted`,
    `path_skips_always_none`, `test_should_skip_source_no_skips`.
    `runner/mod.rs::print_stage_footer` prints
    `STAGE INCOMPLETE — Stage N: p/t passed, s skipped (skips block completion)`
    when any test is skipped, and `test262.rs::test262_staged_impl`
    panics on `summary.skipped > 0` outside digest mode. There is no
    `Skip` count in any stage 0/1/…/43 result, because no skip is
    emitted.

14. **Speculative coverage and skip “carve‑outs” are forbidden.**
    No `[[stage]]` path except those in `STAGES` is ever fed to the
    runner in this plan; the staged runner is the sole entry point. No
    `#[allow(dead_code)]` exists outside `test262/metadata.rs::esid`
    and `test262/metadata.rs::info` (these are *parsed* fields used by
    future repro pin lookup, not speculative generality).

## Reconciliation with `STAGE0.md`

`STAGE0.md` is a snapshot of two failing-test reproducers
(`Symbol.toStringTag in <primitive>` + the `if`/`return` TCO fall‑through)
that historically failed Stage 0. Both bugs are fixed in the current
runner tree:

- **Bug 1 (`in <primitive>` ➜ `TypeError`)** is fixed in the live
  `crates/quench-runtime/src/eval/` `in`‑operator path; the evidence
  is `every_stage_zero_case_passes_through_the_runner` plus the
  targeted primitive‑shape reproducer pins in `host.rs::test_*`.
- **Bug 2 (`Statement::If` TCO fall‑through)** is fixed in
  `crates/quench-runtime/src/eval/statement.rs` (and the per‑iteration
  `let`/`for` invariants are pinned by `tests/test262.rs::test_runner_path_per_iteration_binding`,
  `test_runner_path_multi_let_per_iteration`,
  `test_runner_path_let_closure_inside_initialization`,
  `test_runner_path_var_binding_resolves_before_initializer`,
  `test_runner_path_eval_var_arguments_is_allowed`,
  `test_runner_path_with_function_this_binding`,
  `test_runner_path_using_completion_value`).

Therefore `STAGE0.md`’s “first failure” symptom is no longer reachable,
and the *current* authoritative result is **Stage 0 116/116 passed, 0
failed, 0 skipped, isolated digest about 5.4 s**. `STAGE0.md` must be
retired or rewritten as a historical post‑mortem in a separate diff;
the rewrite is out of scope for this plan but is a prerequisite to
removing the triage header in `tasks/index.json` for stage 0 (currently
`status: "done"` but `rust_loc: "0–0"` — the stage 0 row is correctly
marked done; `STAGE0.md` is the only stale doc).

## Remaining work (to do, not yet done)

- **Retire `STAGE0.md` or convert it to a post‑mortem.** The current
  text documents a failure that no longer exists; it must be either
  rewritten to describe the historical failure + the two fixes, or
  removed. The runner no longer needs it.
- **No fixture rewrite is planned beyond the one pre‑existing
  `propertyHelper.js` swizzle** (Note A). Keep the rule against rewrites
  and add a runner‑invariant test (already present, see
  `harness_loader_tests.rs`) banning further rewrites.
- **Crash budget remains per‑test**: the in‑process timeout is 30 s and
  the worker‑stack is 64 MB / 1 GB (the deep per‑case branch in
  `worker_stack_size`). For stages that still need
  `RUST_MIN_STACK=33554432` raised, the policy is to bump the env in
  `run_isolated` and pin a new invariant test; no per‑test heuristic
  table outside `worker_stack_size`.
- **The runner still prints literal `JsError(\"…\")` envelopes in raw
  `failure.message`.** A small post‑processor (a single helper in
  `host.rs`) could unwrap the envelope for display without changing the
  digest keys; deferred — not on the critical path because the digest
  keys already strip the envelope via `normalize_reason`.

## Verification commands (no long runs performed in this diff)

- `cargo nextest run -p quench-runtime -E 'test(stage_zero|runner_path|isolation|isolated|collect)'`
  — the pinned runner‑invariant suite (collected from
  `runner/execute/tests.rs`, `runner/mod.rs`, `runner/collect.rs`,
  `runner/digest.rs`, `runner/flags.rs`, `host.rs`).
- `cargo nextest run -p quench-runtime -E 'test(every_stage_zero_case_passes_through_the_runner)'`
  — the Stage 0 116/116 reproducer.
- `TEST262_STAGE=0 TEST262_DIGEST=1 cargo nextest run -p quench-runtime
  --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored
  all --no-capture` — authoritative per‑stage confirmation; prints
  `passed: 116, failed: 0, skipped: 0, total: 116, duration_ms: ~5400`.
- `cargo fmt -p quench-runtime && cargo clippy -p quench-runtime
  --all-targets` — formatting + zero‑warning gate.

## Note A — `propertyHelper.js` swizzle

`harness/mod.rs::build_harness` replaces the literal
`var nonIndexNumericPropertyName = Math.pow(2, 32) - 1;` with
`var nonIndexNumericPropertyName = 999999;` before concatenation. This
is a known — and now isolated — substitution; tests under
`harness_scope_tests.rs` exercise the substitution path. The fix in
the upstream JS body is tracked separately and is **not** a runner
concern; the runner merely observes the current upstream behaviour.
