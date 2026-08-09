# Compatibility acceleration audit and execution loop

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Goal

Reduce the time from an observed Node mismatch to a verified general fix by
2–5x, while preserving the full compatibility objective, readable polyfills,
local-only verification, and authoritative evidence.

## Ranked findings

1. **Scope is not mechanically closed.** The task index has 19 records, but
   completion is not linked to current verification evidence. A green focused
   suite can therefore hide unfinished API, fixture, host, and application
   work.
2. **Evidence is fragmented.** Inventory, focused metrics, differential
   reports, task status, and application stages require separate commands and
   can be stale independently.
3. **The differential corpus is the highest-leverage missing data source.**
   The existing report currently contains thousands of non-matches, but queue
   output is not automatically part of the goal audit and report freshness
   must be checked before triage.
4. **Application gates are under-instrumented.** Real npm applications are
   represented by focused stages, but there was no dedicated release-facing
   command to run only those gates quickly.
5. **Parallel stage execution is unsafe for shared-path fixtures.** The
   parallel runner is useful for isolated stages, while repository-relative
   filesystem fixtures can race or leak artifacts. Parallel output must remain
   diagnostic; the serial runner remains authoritative unless isolation is
   proven.
6. **Failure data lacks a fast implementation loop.** A developer needs a
   stable signature, owner, representative fixtures, a focused regression, and
   the smallest relevant gate in one bounded cycle.

## Implemented improvements

- `tools/compat-goal-audit.sh` and `.cjs` now combine task status, focused
  metrics, inventory, differential evidence, and release-gate data into a
  ranked JSON report. Missing evidence is reported explicitly.
- `tools/check-application-stages.sh` runs maintained application gates without
  a full sweep; the release set must cover all six workload classes.
- `tools/compat-queue.sh`, `tools/compat-decision-report.sh`, and the
  differential runner remain the implementation queue: they cluster failures,
  detect stale reports, compare regressions, and preserve representative
  fixtures.
- The serial focused runner remains the authoritative gate for shared-path
  stages; the parallel runner is an optimization and diagnostic instrument.

## Required execution loop

1. Run `tools/compat-goal-audit.sh` and resolve missing/stale evidence.
2. Generate or refresh the scoped differential report.
3. Select the highest-volume owned signature with
   `tools/compat-queue.sh`.
4. Add one readable focused regression and implement the smallest general
   polyfill or host change.
5. Run the focused stage, application gates, affected upstream fixtures, and
   `git diff --check`.
6. Update the owning task with evidence and repeat until the task index and
   release gates are actually complete.

## Status

In progress. The audit and application-gate tools are implemented and pass
their syntax and live application checks. The latest application snapshot is
green for stages 2047, 2069, 2080, 2081, 2104, and 2251. Current audit evidence
still shows 16 unfinished task records, focused retries/failures, and a stale
large non-match corpus, so the compatibility objective remains open.

## Compatibility work recorded through this loop

- Crypto implementation and evidence are tracked in task 005: the obsolete
  SHA-384/SHA-512 fallback was removed, invalid HMAC digest errors were aligned,
  and stage 2553 plus the authoritative hash/HMAC fixtures were added.
- Stream pipeline implementation and evidence are tracked in task 019: stage
  2554 verifies that ordinary function stages receive the preceding stream;
  the authoritative pipeline fixture passed in isolation after the fix.
- HTTP server request sequencing and stream-destroy callback lifecycle remain
  explicitly open. Their current authoritative failures are not being hidden
  behind focused-stage or application-gate results.

## 2026-08-09 continuation checkpoint

- Refreshed the full parallel differential against the current checkout:
  4,682 fixtures, 1,209 matches, 2,116 quench failures, 580 output
  mismatches, 419 both-failed cases, and 108 timeouts.
- Reproduced the stream queue with upstream fixtures. `Readable.resume()` was
  not starting `_read()` for an empty buffer, which prevented auto-destroy
  callbacks and backpressure progress.
- Commit `b54fe4d36` fixes that lifecycle path. `test-stream-auto-destroy.js`
  and `test-stream-backpressure.js` now pass, while
  `test-stream-iter-consumers-text.js` remains open for a separate iterator
  issue.
- The fix was formatted, pushed, and validated with the focused stage 2554;
  application gates remain green.

- Commit `7723557fc` aligns `stream/iter` text consumers for Latin-1,
  invalid UTF-8, and BOM handling. The upstream text-consumer fixture passes.
- Commit `0d4cc5810` normalizes broadcast iterator writes to byte chunks and
  applies drop-oldest/drop-newest policies in synchronous writes. The basic
  and backpressure broadcast fixtures now pass.
- Commit `2bda2538c` implements lazy cached `stream/iter.shareSync()` with
  cancellation and source-error propagation. `test-stream-iter-share-sync.js`
  now passes; `toReadable()` remains a separate open cluster.
- Commit `725a6d6da` completes the byte-mode `toReadable()` and
  `toReadableSync()` adapters. It adds lazy synchronous-source pumping,
  readable-event backpressure, source-error/abort propagation, iterator
  cleanup, and async-generator transform support. The full upstream
  `test-stream-iter-to-readable.js` fixture passes.
- Commit `b4fde24d8` adds synchronous gzip, deflate, Brotli, and Zstd
  `zlib/iter` transforms and preserves Buffer identity from `bytesSync()`.
  The full upstream `test-stream-iter-transform-sync.js` fixture passes.
- Commit `12e8e1c70` moves the standalone PBKDF2 validation helpers into
  `crypto-head.js`; `crypto.js` is now 495 lines and the PBKDF2/HMAC fixtures
  remain passing.
- The Buffer copy/concat validation helpers are now isolated in
  `copy-head.js`; `copy.js` is 496 lines. The size gate is down to 21 remaining
  oversized files, and stages 1019 and 1026 pass after the split.
- The six maintained application gates (Ajv, debug, Chalk, ms, Prettier, and
  `process.argv0`) pass in parallel. The remaining upstream HTTP agent cluster
  is reproducible: `test-http-agent-destroyed-socket.js` reports one extra
  callback, while `test-http-agent-maxsockets-respected.js` and
  `test-http-agent-scheduling.js` each miss a callback. An initial idempotent
  response close-release experiment was reverted because it did not change
  those outcomes; the lifecycle issue remains open for focused investigation.
