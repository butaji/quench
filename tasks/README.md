# Executing the rewrite queue

[`index.json`](index.json) is the sole authority for task identity, status,
dependencies, lane membership, pinned revisions, and `next_task`. Task files own
implementation intent and acceptance evidence; they do not redefine queue state.

Start with `next_task`. A task may start only after every `depends_on` item is
`done`. Independent lanes may proceed in parallel, but task 24 is the explicit
convergence gate and task 27 is the only production cutover. Pre-cutover work
uses separately named development binaries; production must never choose an
engine through a flag or environment variable.

Statuses are `pending`, `in_progress`, and `done`. At most one task is
`in_progress`; it must be `next_task`. On completion, retain its Markdown file,
mark it `done`, and advance `next_task` to the first ready task in lane order.
Set `next_task` to `null` only when every item is done.

## Shared completion rules

- Follow the [repository rules](../AGENTS.md), including semantic separation,
  benchmark integrity, exact fallback, and Apple M4/macOS qualification.
- Preserve observable values, descriptors, identity, ordering, errors, exit
  status, output, and host effects. Changed Node behavior is checked against the
  local Node oracle and relevant pinned upstream source.
- Keep one authoritative value, heap, object, activation, opcode, and host-root
  representation. Derived metadata must be generated or validated from it.
- Treat allocation failure, malformed residual data, interruption, re-entry,
  and unsupported platform mechanisms as explicit checked transitions.
- Store raw measurements and large reports under ignored `target/` paths with
  source, binary, toolchain, host, and command provenance.
- Run focused tests first, then every suite named by the task. A command that
  discovers no tests is a failure, not evidence.
- Performance measurements never replace correctness evidence. Functional work
  may complete without a speed claim unless the task explicitly owns a
  performance gate.

## Conformance ratchet

Task 19 freezes per-test legacy outcomes for Test262, Wasm, and
`tests/node-compat`. From then on every change that touches the next engine,
the host facade, or a runner is checked against the latest recorded pass set
for each suite it can affect: newly failing tests are regressions and block the
change. Feature tasks close on their mapped conformance slices (task 20 maps
Test262 stages to tasks 11–18), so completion is measured, not asserted.

A foundation task (07–10) closes on its mechanism contract. Producers in later
tasks adopt it as part of their own definition of done, which keeps the
dependency graph acyclic.

## Final gates

- Every official test discovered from the pinned Test262 checkout passes; there
  is no Quench-owned feature skip list.
- Every directive discovered from the pinned WebAssembly testsuite passes.
- Every fixture under `tests/node-compat` discovered by `quench-node-test`
  matches the local Node oracle.
- The standalone interpreter meets the pinned `../v2` reference on both median
  Score and median maximum RSS for Richards, DeltaBlue, Crypto, and Splay.
- The final tree contains no guest JIT, copy-and-patch stencil, executable-memory
  runtime, legacy backend, or sibling-checkout build dependency.

The generic Lisp-mindset skill suggests size caps, but this repository's rules
explicitly reject mandatory line-count or complexity ceilings. Cohesion and
reviewability remain required; no numeric cap is imposed.
