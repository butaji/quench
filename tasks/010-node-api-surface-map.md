# Node API surface map and 100% coverage strategy

## Goal

Define the complete Node API surface that `quench-node` must support, map every
surface area to its current coverage and planned work, and lay out the strategy
that takes the project from the current 506 focused-stage pass mark to a
verifiable 100% of the Node API.

## Current state (baseline)

- Focused-stage suite: 506 contract gates pass under
  `tools/check-focused-stages.sh`.
- Polyfill modules registered in
  `crates/quench-node/polyfills/bootstrap.js`: 32 (see `grep "if (name ==="`).
- Canonical Node built-in module list (`require('module').builtinModules`)
  contains 60 modules (excluding the private `_http_*` family).
- Upstream parallel test directory: 4684 files under
  `tests/node/test/parallel/`.

## Surface model

Three concentric layers, each tracked separately:

1. **Modules** — the 60 built-in modules from `module.builtinModules`. Each
   module has a `if (name === "...")` branch in `bootstrap.js`, or a TODO. Some
   modules map onto host functions exposed by `crates/quench-node/src/main.rs`.
2. **Globals** — names available on `globalThis` without `require`:
   `process`, `Buffer`, `console`, `queueMicrotask`, `setTimeout`,
   `setImmediate`, `setInterval`, `clearTimeout`, `clearImmediate`,
   `clearInterval`, `TextEncoder`, `TextDecoder`, `URL`, `URLSearchParams`,
   `structuredClone`, `navigator`, `performance`, `fetch`, `crypto`, `events`,
   `AbortController`, `AbortSignal`, `DOMException`, `atob`, `btoa`,
   `globalThis`, `WebAssembly`, etc.
3. **Upstream fixtures** — the 4684 parallel fixtures. Each fixture is a Node
   contract gate; a focused stage is the project's compact rephrasing of one
   fixture (or a cluster of fixtures).

## Coverage strategy

The path to 100% has three legs. Each leg is itself a `tasks/*` file with its
own backlog.

1. **Module surface (task 011).** Walk the 60 `builtinModules` and either
   build a polyfill branch for each missing one, or mark the module as
   "unsupported on this target" with an explicit `Error("No such built-in
   module: …")` so `require('…')` returns the Node-correct shape. No fixture
   passes without a registered module.
2. **Global surface (task 012).** Walk the Node `globalThis` names and
   document/cover each. Most already exist via `bootstrap.js`; the gaps are
   `fetch`, `navigator`, `WebAssembly` extras, `BroadcastChannel`,
   `CustomEvent`, `Event`, `EventTarget`, `MessageChannel`, `MessagePort`,
   `ReadableStream`/`WritableStream`/`TransformStream` polyfills (often
   exposed via the host or via JS emulation), and the `URL` quirks.
3. **Upstream fixtures (task 013).** Walk `tests/node/test/parallel` cluster
   by cluster. For each cluster, write focused stage(s) that capture the
   Node-accurate contract, and then implement the smallest polyfill behavior
   that makes the stage pass. Run the up-stream fixture directly with
   `tools/run-node-tests.sh` to confirm parity with Node.

The order is: module surface first (so the polyfill is registered), then
global surface (so `globalThis.X` is available), then fixtures (so the
behaviour matches).

## Host (Rust) surface

The 466-line `crates/quench-node/src/main.rs` exposes host callbacks as
`globalThis.__quench_*`. The pattern is intentionally thin: most Node
behaviour lives in JS, and Rust only provides the unsafe / OS-bound
primitives. The remaining Rust work is:

- `__quench_script_source` (one-line) so the cluster polyfill can re-evaluate
  the entry source in worker mode.
- `__quench_*` host helpers for: spawn/exec, signal handling, real TCP/UDP
  sockets, real DNS, real process IPC, real fs handle pull APIs, real
  process killing, real `os.cpus`, real `os.networkInterfaces` device names,
  real `process.memoryUsage()`.
- A per-context permission gate for fs writes when the user requests
  `--allow-fs-write`.

These are tracked in `tasks/014-host-surface.md`.

## Measurement protocol

Every slice reports two numbers:

- `tools/check-focused-stages.sh` (compact, deterministic, fast).
- `tools/measure-node-tests.sh` over a sub-directory, scoped to the slice's
  API cluster, to count raw pass rate against the upstream fixture.

No slice is "done" if either number regresses.

## Retrospective protocol

At the end of every slice, the task file's `## Status` block records:

1. Diff footprint (`git diff --stat`).
2. Before/after pass count for focused stages and the relevant upstream
  cluster.
3. One thing that made the slice slower than it should have been, and the
  specific process change to fix it on the next slice.

## Done when

- Every `builtinModules` entry either has a polyfill branch or an explicit
  "not supported" error.
- Every Node `globalThis` name that the polyfill exposes passes its
  contract gate.
- `tools/measure-node-tests.sh tests/node/test/parallel` reports ≥ 95%
  (some up-stream fixtures require network, threading, or addons and are
  expected to skip).
- The focused-stage suite reports 100% pass on the registered gate set.

## Status

In progress. The 32 → 60 module gap and the upstream fixture coverage are
the active backlog; see `tasks/011`–`tasks/014`.
