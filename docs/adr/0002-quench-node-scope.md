# ADR 0002: `quench-node` Node-API compatibility host

- **Status:** accepted
- **Date:** 2026-08-18
- **Scope:** adds a Node.js-API compatibility host to the Quench workspace.
  Does **not** amend the primary goal (`GOAL.md`: 100% stable test262
  conformance) and does **not** amend the frozen doctrine in `AGENTS.md` and
  `docs/architecture.md`.

## Context

`quench-runtime` is a pure JavaScript engine that ships test262 conformance as
its sole product goal. Real-world use of a JS engine almost always means
running programs that link against Node-style builtins (`fs`, `path`, `http`,
`stream`, `Buffer`, `process`, `events`, …) and against npm packages that
resolve through `require()` / `import` with a `node_modules` walk. The current
runtime knows nothing about any of this: by design, rule 5 of `AGENTS.md`
("Runtime isolation is absolute") forbids it from learning about hosts, and
`docs/architecture.md` forbids a "self-hosted JavaScript builtin layer."

This ADR records the v1 shape of a Node.js-API compatibility surface built
**on top of** `quench-runtime`, in a separate crate, with its own test
runner, against an upstream fixture tree. It is the only piece of work in
this repo allowed to know what "Node" is.

## Decision

### 1. A new host crate, peer of `quench-test262`

A new workspace member `crates/quench-node/` consumes `quench-runtime` as a
pure JS engine and **is itself** a host, in the same sense that `quench-test262`
is a host. The two hosts share no code; each owns its own host glue. There
is no `crates/quench-host/` extraction in v1 — the duplication is real but
tiny (eval-from-string, microtask drain, completion → exit code), and a
shared host crate would require refactoring the in-flight test262 runner,
which rule 8 of `AGENTS.md` ("Fix workflow: fix families of related failures,
not single cases") forbids as speculative work.

### 2. A new test-runner crate, owning the Node fixture tree

A new workspace member `crates/quench-node-test/` owns:

- the Node.js test fixture source as a git submodule pinned at
  `crates/quench-node-test/node-tests/` (URL:
  `https://github.com/nodejs/node`, path `test/`);
- the runner that discovers, composes, and executes those fixtures through
  the host contract;
- the completion classification that maps a host run to pass / fail / skip /
  crash.

This mirrors the `tests/test262/` submodule and `crates/quench-test262/`
split exactly. `quench-node-test` may never modify the upstream fixture
tree, may not rewrite, shim, or short-circuit any Node harness behavior, and
may not depend on the Node API surface in a way that influences fixture
outcomes. `quench-node` knows nothing about the fixture tree, the runner,
or Node test policy — same isolation rule that already applies to
`quench-runtime` with respect to test262.

### 3. All Node builtins in Rust; no self-hosted JS bootstrapper

Every `node:` module is implemented as Rust code in `quench-node` and
installed via the same `builtin!` / `value!` / `heap!` mechanism the
runtime already uses for its own intrinsics. The host owns the *policy* of
which builtins get installed; the runtime owns the *mechanism* of
installation. There is no self-hosted JavaScript builtin layer — that
form is explicitly forbidden by `docs/architecture.md`.

The Node builtins are described as **data + patterns + machines + effects**:

- **Data** declares each module, export, signature, error class, and
  intrinsic relation, in a single canonical table — exactly the
  `builtin_table!` shape `docs/architecture.md` already names.
- **Patterns** (`sync`, `async`, `callback`, `promise`, `emitter`,
  `resource`, `transform`, `stream`) are *codegen templates* the
  `node!` macro recognizes and lowers into one of a small number of Rust
  function templates. They are not runtime values; they do not exist as a
  second dispatch layer.
- **Machines** (`Resource`, `Async`, `Stream`, `Process`, `Module`) are
  handwritten Rust functions in `crates/quench-node/src/machines/`. They
  are the only place where state-machine code lives, and they are
  shared across the builtins that need them.
- **Effects** (`fs.read`, `fs.write`, `net.connect`, `timer.wait`,
  `process.spawn`, …) are thin Rust wrappers over the kernel crates
  selected per `docs/architecture.md`'s table. Effects never appear in
  the runtime's type system; they are visible only inside the host.

The generated Rust is checked in under `crates/quench-node/src/generated/`
and is a build artifact of the table. Readable Rust owns the
algorithms — the HTTP parser, the WHATWG URL parser, the `Buffer.from`
argument-shape dispatch, the `zlib` stream state — exactly the split
`docs/architecture.md` mandates.

### 4. Kernels live in `quench-node`, not in the runtime

`quench-node` adds its own kernel dependencies for the Node-specific
surface that the runtime has no reason to need: `mio` (or `polling`) for
I/O readiness, an HTTP parser (`httparse` and the `h2` crate as needed),
TLS via `rustls` (pure-Rust, no OpenSSL), SQLite via `rusqlite`, and
`flate2` for `zlib`. Each kernel is wrapped behind a semantic adapter so
the JS-visible surface remains runtime-owned; no kernel is exposed as a
second runtime. The runtime's existing kernel table (`regress`, `chrono`,
`num-bigint`, `serde_json`, `urlencoding`, ICU4X) is reused verbatim
where it covers the same surface (`url`, `querystring`, `path`, …) — no
new kernel where an existing one already does the job.

A shared `crates/quench-kernels/` workspace member that both the runtime
and the host depend on is **deferred**: extracting it would require
refactoring the runtime, which is the in-flight family of module-graph
semantic fixes the previous coordinator is working on. We do not touch
in-flight runtime code without a real semantic reason; the Node host is
not that reason.

### 5. v1 module set: Bun's 🟢-only set

The v1 module and global set is the **🟢-only set** from Bun's
`nodejs-compat` matrix at
<https://bun.com/docs/runtime/nodejs-compat> (Node v26):

`assert`, `buffer`, `console`, `dgram`, `dns`, `events`, `fs`, `http`,
`http2`, `net`, `os`, `path`, `punycode`, `querystring`, `quic`,
`readline`, `stream`, `string_decoder`, `timers` (+ `timers/promises`),
`tty`, `url`, `zlib`, `trace_events`, `sqlite`; plus the 🟢 globals
(`Buffer`, `URL`, `fetch`, `TextEncoder`, `setImmediate`, `AbortController`,
`Event`/`EventTarget`, `MessageChannel`/`MessagePort`, `BroadcastChannel`,
`WebAssembly`, `performance`, `queueMicrotask`, `structuredClone`,
`atob`/`btoa`, `require`, `process` partial).

The 🟡 set is tracked as a separate v2 backlog; the 🔴 set
(`node:sea`) is out of scope indefinitely.

### 6. Module resolution: full `require()` + `import` for real npm

v1 ships a real module resolver: a `node_modules` walk, an `exports` /
`imports` map, conditional exports with the `import` / `require` /
`node` / `default` conditions, CJS↔ESM interop, `createRequire`,
`require.cache`, and `require.resolve`. The "servers + libraries" tier
(Hono, Express, Fastify, Koa, `ws`, `graphql-yoga`, Prisma-style
clients, `dayjs`, `lodash`, `zod`, …) is the criterion; framework CLIs
and build steps (Next/Nuxt/SvelteKit dev mode, `next build`) are **not**
v1.

This scope is necessary because the stated v1 criterion — "the most
popular web apps and their npm dependencies must run" — fails on a
slimmer module resolver. The cost is several months of work concentrated
in the resolver; the alternative (defer to v2) was rejected.

### 7. Event loop: single-threaded with `spawn_blocking` + `mio`

The Node host runs single-threaded. Blocking syscalls (`fs.read`,
`fs.write`, `child_process.spawn` sync) execute on a `spawn_blocking`
thread pool; I/O readiness (`net`, `http`, `dgram`, `fs` watchers) runs
on the main thread via `mio` (epoll/kqueue/IOCP). The microtask drain,
the `setImmediate` queue, the `process.nextTick` queue, and the timer
wheel are all driven from a single loop. CPU-bound work (`zlib`,
`crypto`, `hash`) is offloaded to a worker-thread pool that delivers
results back as microtasks on the main thread.

A multi-isolate scheduler with shared heaps and per-isolate event loops
("real `worker_threads`") is **not** v1. It would force the runtime to
know about threads, which rule 5 of `AGENTS.md` forbids.

### 8. v1 acceptance criterion: a curated green-list of npm scripts

A hand-picked set of 20–50 real packages from the "servers + libraries"
tier plus the 🟢 `node:` modules must each run from a published npm
version under `quench-node ./script.js` with a passing exit code. Skips
are allowed but must be listed and justified. The order in which
packages are added to the green-list is the v1 plan; the per-package
acceptance is the gate. The green-list lives in
`docs/NODE-STAGES.md` and is a definition document, not a status
ledger — same shape as `docs/STAGES.md`.

### 9. Performance is explicitly deferred

"Beat V8 on web apps" and "lowest mem/RSS in the Quench family" are
**not** v1 acceptance criteria. They are downstream aspirations
captured here so the work is not forgotten, but they do not appear in
the worker loop, the quality gate, or any stage definition. The
deferral is necessary because:

- the frozen doctrine in `AGENTS.md` ("Generic semantics precede
  guards; every guard falls back to the same generic operation
  without changing ordering or observability") and the implementation
  order in `docs/architecture.md` ("Complete slow semantics and cheap
  `Unknown` behavior precede guarded fast paths") are incompatible
  with making perf a primary goal in this phase;
- the only v1 success criterion that the existing team topology in
  `GOAL.md` can actually measure is test262 conformance, and adding a
  second primary goal without a written tie-break rule would let
  either goal silently regress the other;
- the residual-VM bet is that a facts-first design is *cheaper to
  maintain* than V8, not that it is faster cold; the perf claim is
  downstream, and only after the generic path is complete and
  benchmarked.

When the v1 green-list is complete, a separate ADR will set the perf
targets, the workload mix, the allowed conformance regression budget,
and the conflict tie-break rule. Until then, no worker assignment may
optimize for perf at the expense of conformance.

## Consequences

### Positive

- The runtime is untouched, the test262 campaign is untouched, the
  frozen doctrine is untouched. The Node host is purely additive.
- The boundary between "JS engine" and "Node host" is mechanically
  checkable: `quench-runtime`'s `Cargo.toml` does not gain Node-shaped
  dependencies, and `tools/check-boundaries.sh` can be extended to
  verify that.
- The macro+patterns+machines+effects shape is the doctrine's own
  recommendation applied to a new surface; no new abstraction is
  introduced.
- A real module resolver is built once, in one crate, with no shared
  scaffold to negotiate.

### Negative

- The 🟢-only set is large. The v1 budget is measured in months, not
  weeks.
- A real module resolver is large. Several months of concentrated
  resolver work are needed before any non-trivial npm package runs.
- The "almost no handwritten implementation" framing is aspirational
  for the modules that are mostly data (`fs.readFile`, `path.join`,
  `events.EventEmitter`'s `on`/`off`/`emit`/`once`) and is *not* true
  for the modules whose observable behavior is an algorithm (`Buffer`
  encodings, WHATWG URL parser, HTTP parser, `zlib` stream state).
  The ADR's honest framing is "declarative metadata + small
  handwritten algorithms + a small set of named state machines +
  generated wrappers." The plan files say this so reviewers don't
  expect zero handwritten code.
- The 🟡 Bun items (`worker_threads`, `tls`, `https` server, `cluster`,
  `vm` options, `inspector` Session beyond the `Profiler` domain,
  `wasi` extensions, `perf_hooks` Node-only entries) are not in v1.
  Any npm package that *requires* a 🟡 surface will not run; this is
  called out in the green-list.

### Neutral

- `crates/quench-host/` extraction is a possible v2 refactor once both
  `quench-test262` and `quench-node` are stable. It is not v1 work.
- `crates/quench-kernels/` extraction is similarly a possible v2
  refactor. It is not v1 work.

## References

- `AGENTS.md` — frozen doctrine, team topology, linter law.
- `GOAL.md` — primary goal, worker loop, consolidation loop, quality
  gate.
- `docs/architecture.md` — runtime architecture, the
  `builtin_table!` example, the kernels table.
- `docs/STAGES.md` — test262 stage map (template for
  `docs/NODE-STAGES.md`).
- Bun's `nodejs-compat` matrix,
  <https://bun.com/docs/runtime/nodejs-compat> (Node v26) — the
  source of the v1 module and global set.
