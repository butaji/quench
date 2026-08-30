# AGENTS.md — `quench-test262`

## Determinism contract

The harness runner must produce the same per-test outcome whether each test
is dispatched in isolation, run sequentially through one runner, or run
concurrently across threads. This is pinned by unit guards in
`src/runtime_host_determinism_tests.rs` and the binary
`src/bin/compare-runs.rs`.

## Determinism verification

Determinism is verified by the unit guards and `compare-runs` binary described
below. Do not record pass counts, stage totals, completion percentages, or
failure inventories here; those results belong to runner output and commit
history, consistent with repository rule 16.

## Determinism invariants

- `discover_js_files` returns paths in sorted order — file order is stable.
- `module_graph::dependency_order` is a deterministic post-order DFS.
- `StageReport::outcomes` reconstructs a per-path outcome map from a batch
  report; only failed tests appear in `failures`, missing paths are passes.
- `LinkedModuleGraph::execute` sets/clears `CURRENT_MODULE_GRAPH` and
  `CURRENT_MODULE_ID` around the call; `execute_with_context` resets
  locals, environments, private slots, globals, and the promise microtask
  queue at every entry.
- `compare-runs` per-test timeout (`10 s`) is generous enough to absorb CPU
  contention on the heaviest tests; divergences inside the timeout are real
  order/thread bugs.

## Compare-runs binary

`src/bin/compare-runs.rs` runs the same discovered file list in three modes
and diffs the per-test outcomes:

- **individual**: one fresh `RuntimeHost` + `HarnessCache` per test file.
- **sequential**: one `RuntimeHost` + `HarnessCache`, files iterated in order.
- **parallel**: one `RuntimeHost` + `HarnessCache` per worker, files split
  into `WORK_BATCH = 32` chunks via a shared atomic index.

Shared workload queue is `Arc<Mutex<usize>>` updated with `+= WORK_BATCH`
per claim. Aggregated outcomes are merged via `Arc<Mutex<Outcomes>>`.

`STACK_SIZE = 512 MiB` per worker thread to fit deep parser/reducer
recursion, matching `run-stages` and `triage`.

`PER_TEST_TIMEOUT = 10 s` per test via a detached `std::thread` + `mpsc` channel.
Tests that exceed the timeout are recorded as failures with reason
`"test execution timed out"`; the abandoned thread is `std::mem::forget`-ed
so the process can exit. The `std::process::exit` call at the end of `main`
guarantees no leftover thread blocks the comparison.

## Test layout

- `src/lib.rs` — runner contract, `TestMetadata`, `StageReport`, `TestOutcome`.
- `src/harness_cache.rs` — file-path-keyed harness source cache.
- `src/runtime_host.rs` — `RuntimeHost` adapter, `LinkedModuleGraph`.
- `src/module_graph.rs` — deterministic module graph with edge labels.
- `src/bin/run-test.rs` — single-file runner.
- `src/bin/run-stages.rs` — staged runner over `docs/STAGES.md`.
- `src/bin/triage.rs` — bucketed failure triage with parallel workers.
- `src/bin/compare-runs.rs` — three-mode determinism comparison.

## Test262 harness fidelity

`quench-test262` is the sole owner of test262 metadata, harness composition,
and runner contracts. It must not override, rewrite, shim, or replace any
test262 harness behavior — only load the declared harness sources, compose
them as test262 specifies, and execute through the host contract.
