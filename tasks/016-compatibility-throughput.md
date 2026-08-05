# Compatibility throughput and differential triage

## Goal

Increase compatibility implementation throughput by 2–5x without weakening
the readable-polyfill, local-verification, or no-GitHub-CI requirements.

The current one-mismatch-at-a-time workflow is useful for minimizing
regressions, but it serializes discovery, diagnosis, implementation, and
verification. The next tooling investment is a differential test pipeline
that compares Node and quench-node, clusters failures, and produces an
owned work queue.

## Workstreams

### 1. Differential corpus runner

Add a local tool that executes each selected upstream fixture with both Node
and quench-node and records:

- fixture path;
- exit status;
- normalized stdout and stderr;
- timeout or crash state;
- first failure signature;
- API cluster and owner, when known.

The runner must support deterministic timeouts, bounded child-process cleanup,
repeatable ordering, and persisted results. It must not embed JavaScript source
inside JavaScript string literals.

### 2. Failure clustering and triage

Normalize failures into stable categories:

- missing module or API;
- assertion/value mismatch;
- exception type, code, or message mismatch;
- asynchronous ordering or timing;
- serialization/encoding mismatch;
- environment or platform limitation;
- flaky or nondeterministic result.

Emit a ranked queue of unique signatures rather than requiring a developer to
rediscover the first failing fixture manually. Store minimized reproductions
as readable focused stages.

### 3. Cluster-sized slices

Keep focused files below the enforced size and complexity limits, but group
related cases into one API slice where practical. A slice may contain several
readable stage files covering one cluster such as URL parsing, streams/events,
filesystem/path, crypto/network, or module loading.

Do not use one-file-per-mismatch as a hard rule when the failures share one
general implementation. Retain one focused regression per distinct contract.

### 4. Generated scaffolding

Add a stage template/generator that creates source files, imports the shared
Node test helpers, adds exit-event assertions, and updates the ledger without
generating opaque JavaScript strings. Generated files remain ordinary,
formatted, reviewable JavaScript.

### 5. Parallel ownership

Partition work into up to five non-overlapping API streams:

1. URL/WHATWG and encoding;
2. streams/events and async ordering;
3. filesystem/path/process/module loading;
4. crypto/network/OS integration;
5. harness, globals, and test infrastructure.

Each stream must use an isolated worktree or branch, own distinct files, and
merge only verified commits. Shared polyfill edits require explicit ownership
handoff to avoid conflicting changes.

### 6. Coverage and retrospectives

Extend local reports to show:

- upstream fixtures executed;
- pass, fail, skip, timeout, and platform-limited counts;
- pass rate by API prefix/cluster;
- unique failure signatures;
- regressions since the previous run;
- completed and unassigned work items.

Do not call this an API percentage. Node tests are not one-to-one with Node
APIs; the API percentage remains `unmeasured` until an explicit API inventory
and mapping exist.

## Execution order

1. Build the differential runner on a small representative corpus.
2. Add normalized result persistence and failure clustering.
3. Add prefix/cluster reports and regression comparison.
4. Add stage scaffolding generation.
5. Partition the backlog across isolated workstreams.
6. Batch related failures into general polyfill slices.
7. Re-run the full corpus locally and record a retrospective after each batch.

## Success criteria

- A corpus run produces a deterministic Node-vs-quench result file.
- Repeated failures collapse into stable signatures with representative
  fixtures.
- A developer can select a cluster and receive a bounded, ordered work queue.
- Five independent workstreams can operate without overlapping ownership.
- Full local corpus runs clean up timed-out child processes.
- Focused stages remain Prettier-, ESLint-, and `git diff --check`-clean.
- No GitHub Actions or other GitHub CI is introduced.

## Expected impact

Differential triage should remove the serial “find the next mismatch” cost.
Cluster-sized slices should reduce stage bookkeeping, and isolated ownership
should provide the largest throughput gain when multiple contributors work in
parallel. Together these changes target roughly 2–5x throughput; this is a
working hypothesis to validate with before/after corpus and cycle-time data.

## Status

Planned. Existing tools provide measurement, parallel focused execution,
timeouts, and prefix reports, but do not yet provide the complete persistent
Node-vs-quench differential queue described here.
