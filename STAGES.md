# Node compatibility stages

This is the execution plan for bringing `quench-node` to complete, measured
compatibility with the Node test suite. It is a roadmap for semantic families,
not a list of one-off fixture fixes. The source of truth for fixture behavior
remains the checked-out `tests/node` submodule and the local Node CLI; this file
only records derived inventory, invariants, and gates.

## Completion contract

The target is **100% of the selected upstream `test/parallel` fixtures**, with
the selection and result set reproducible from a clean checkout. A fixture is
complete only when it:

1. executes through `quench-runtime` and `quench-node-test` (never another JS
   engine);
2. exits with the same observable result as the local Node oracle, including
   values, properties/descriptors, callback order, errors, exit status, and
   externally visible I/O;
3. is either `pass` or an explicit, independently justified platform skip that
   Node itself makes under the same capability profile; and
4. is represented in the machine-readable result inventory and the committed
   stage manifest.

An exception, timeout, crash, unsupported-module placeholder, or unexamined
platform branch is not a pass. A fixture that happens to pass because a stub
does nothing is not evidence until its assertions and required effects are
verified against Node.

The final gate must report, without a filename filter:

```text
pass + justified_skip == executable fixtures
fail == 0, timeout == 0, crash == 0, unclassified == 0
```

The result must include the exact submodule commit, host/runtime commits,
platform profile, Node version, timeout, and fixture inventory hash.

## Suite census (derived 2026-08-23)

The upstream `tests/node/test/parallel` checkout currently contains **4,727
`test-*` executable fixtures**: **4,234 `.js`** and **493 `.mjs`**. There is
also one non-fixture Python file and one status file. The checked-in
`crates/quench-node-test/node-tests/parallel.txt` manifest contains 629 entries;
the remaining 4,098 fixtures are not a conformance gate yet. The runner's
recursive discovery must remain the canonical inventory: top-level-only or
`.js`-only enumeration silently understates the target.

Filename prefixes are useful triage evidence, not semantic ownership. A first
partition of the 4,727 fixtures (ordered to avoid double counting) is:

| family | fixtures | dominant upstream prefixes |
| --- | ---: | --- |
| network and HTTP | 1,545 | `http`, `http2`, `net`, `tls`, `dns`, `dgram`, `quic`, `https` |
| filesystem, VFS, and loading | 614 | `fs`, `vfs`, `require`, `module`, `npm`, `watch` |
| process and concurrency | 341 | `child`, `cluster`, `worker`, `atomics`, `message` |
| events, async context, and timers | 304 | `events`, `event`, `async`, `diagnostics`, `domain`, `timers` |
| observability and performance | 282 | `inspector`, `debugger`, `trace`, `v8`, `heap`, `perf`, `snapshot` |
| streams and backpressure | 282 | `stream`, `readable`, `pipe`, `pipeline`, `webstreams` |
| crypto and security | 263 | `crypto`, `webcrypto`, `x509`, `openssl` |
| language, assertions, and errors | 192 | `assert`, `vm`, `module`, `compile`, `eval`, `error` |
| test/harness/process infrastructure | 136 | `runner`, `node-test`, `process`, `common` |
| platform and special modules | 110 | `tty`, `wasi`, `sqlite`, `repl`, `readline`, web globals |
| path, URL, query, util, and OS | 102 | `path`, `url`, `querystring`, `util`, `os`, `punycode` |
| buffers, typed arrays, and encoding | 77 | `buffer`, binary, text, decoder/encoder |
| cross-family/platform-specific remainder | 479 | feature flags, CLI, startup, locale, and regressions |

The counts intentionally overlap conceptually (for example, HTTP consumes
streams and timers) but are disjoint in this triage partition. Every fixture
must eventually have a semantic family and an evidence record; “remainder” is
only a temporary queue.

The most frequently imported public modules provide a second, more reliable
planning signal: `assert` (4,164 references including `node:assert`), `fs`
(790), `http` (479), `child_process` (508), `net` (459), `stream` (376),
`worker_threads` (399), `http2` (304), `tls` (232), `crypto` (307), and
`node:test` (267). Internal imports (`internal/*`, `internal/test/binding`,
and `node:vfs`) are separate compatibility contracts and must not be treated
as ordinary public-module coverage.

## The semantic record

Before adding a host branch, record one declaration in the shared API facts
layer. The declaration is the sole source for registration, wrapper shape,
argument validation, error identity, capability requirements, and evidence:

```rust,ignore
node_api! {
    module: "buffer",
    export: "Buffer.prototype.fill",
    kind: Method,
    receiver: BufferLike,
    args: [value: FillValue, offset: ToIntegerOrInfinity = 0,
           end: ToIntegerOrInfinity = len, encoding: Encoding?],
    result: SameReceiver,
    errors: [ERR_OUT_OF_RANGE, ERR_INVALID_ARG_TYPE],
    effects: [MutateBytes],
    evidence: ["test-buffer-fill.js", "test-buffer-generic-methods.js"],
}
```

The actual macro name and location may evolve, but its fact schema should have
exactly one canonical representation for:

- module/export and aliases (`node:` included);
- callable kind, receiver/construct rules, and property descriptors;
- argument coercion, defaults, validation, and error taxonomy;
- return/identity rules and observable mutation;
- required runtime capability/resource effects;
- `Proven`, `Guarded`, or `Unknown` status and fixture/oracle evidence.

The macro should generate only mechanical consequences: module tables,
capability IDs, registration, ordinary JS wrappers, and focused metadata. The
host handwrites only observable algorithms and edge adapters. OXC remains the
only syntax tree; Quench does not add a second AST. Generated code, static
tables, binary text, caches, and handwritten code all count in the budget.

## Shared model and invariants

The family stages below compose the same small set of mechanisms:

1. **Value facts** — JavaScript coercion, property keys/descriptors,
   prototype/identity, typed-array views, errors, and completion records.
2. **State machines** — emitter subscriptions, stream queues, timers,
   sockets, handles, promises, and module caches as `state = reduce(events)`.
3. **Capability facts** — a bounded operation/resource request at the host edge;
   the runtime owns no Node-specific universe and Node owns no alternate
   semantics.
4. **Boundary adapters** — filesystem, clock, process, resolver, network,
   crypto, and output. Effects are explicit here, not hidden in wrappers.
5. **Evidence facts** — fixture, local Node observation, Node source reference,
   expected outcome, profile, and current status.

Do not duplicate a fact in a Rust handler, a JS prelude, a manifest, and a
test-specific exception. Derive those views from the declaration/evidence
record. Prefer a pattern table or state transition over a growing capability
`if` chain. Keep new functions under 40 lines and files under 500 lines; split
by semantic data boundary when they grow.

## Staged roadmap

Each stage is complete only when its family gate is green and the full runner
still has no regression. A stage may add declarations and generated mechanics
for many APIs, but must not add a fixture-specific special case that has no
reusable semantic rule.

### 0. Measurement and runner truth

**Scope:** `quench-node-test`, inventory, oracle comparison, and stage data.

Make recursive `.js`/`.mjs`/`.cjs` discovery, manifest selection, timeout,
crash isolation, skip classification, and result serialization authoritative.
Add an inventory check that fails if a discovered fixture is absent from a
declared family or if a manifest entry disappears. Keep `run-parallel` useful
for one fixture, one family, and the complete 4,727-fixture gate; never call a
triage printout a compatibility result.

Evidence:

```sh
cargo test -p quench-node-test
cargo run --release -p quench-node-test --bin run-parallel -- --all --timeout-secs 30
# Add --results PATH for a machine-readable inventory/result record.
```

The complete inventory gate uses the optimized profile: large upstream
fixtures such as Buffer UTF-8/index workloads are semantically finite but can
exceed the 30-second bound in an unoptimized debug build. A debug timeout is
diagnostic evidence, not a compatibility result.

Exit criteria: deterministic counts, structured pass/fail/skip/timeout/crash/
unclassified records, fixture hash, and a clean baseline against the local
Node oracle. The result JSON schema is version 2; a child result without an
explicit marker is `unclassified`, never an implicit pass or fail.

### 1. Runtime/value semantics

**Scope:** OXC reduction, calls/constructors, lexical bindings, `this`,
properties/descriptors, prototypes, symbols, arrays/holes, coercion, errors,
promises, and typed-array views.

This is a prerequisite for every API family. Complete language/assert/vm
fixtures that expose these semantics before adding host branches. Consolidate
`ToPrimitive`, numeric conversion, property-key conversion, callable receiver
identity, and error construction into runtime facts. Buffer and stream work
must consume these rules, not reimplement them.

**Gate:** all selected `assert`, `vm`, language/regression, buffer/typed-array,
and generic-object fixtures pass; no host module changes are needed to explain
a runtime-only result.

### 2. Core data APIs

**Scope:** Buffer, typed arrays, DataView, string/byte encodings,
`string_decoder`, `path` (posix/win32), URL/WHATWG URL, `querystring`, `util`,
`punycode`, `os`, and console.

Declare method families by shared argument/return facts (index ranges,
encoding, view offsets, descriptor/identity behavior). Generate aliases and
registration; handwrite byte algorithms, path grammar, URL parser state, and
formatting where observable. Cross-check all coercion/error cases against Node,
not only happy-path focused tests.

**Gate:** the 77 buffer/binary and 102 path/URL/util/OS partition fixtures plus
their imports in other families pass without receiver/name-specific branches.

### 3. Events, scheduling, and async context

**Scope:** `events`, EventTarget, `async_hooks`, `diagnostics_channel`, domains,
timers, microtasks, `process.nextTick`, Abort signals, and promise lifecycle.

Model each as a small transition system over canonical identity. One emitter
registry and one listener record must serve public methods, subclass
construction, static helpers, and internal consumers. Timer handles and async
resources are resources with explicit transitions (`created`, `queued`,
`fired`, `cancelled`, `closed`), not ad hoc callback flags.

**Gate:** all 304 events/async/timer fixtures and dependent callback-order
assertions pass under repeated runs; event order and error propagation match
Node.

### 4. Filesystem, VFS, and module loading

**Scope:** `fs`, `fs/promises`, file handles, stats, watchers, `node:vfs`,
`require`, CJS/ESM/module resolution, package exports, built-in aliases, and
permission/error behavior.

Separate pure path/resolve facts from effectful resource operations. A single
module record/cache and a single resolver fact set must serve CJS, ESM, package
JSON, `node_modules`, and VFS providers. Generate API surfaces from the same
declarations; keep provider-specific I/O at the edge.

**Gate:** all 614 filesystem/loading fixtures pass on a hermetic temporary
root, with no host-home or network leakage and exact Node error names/codes.

### 5. Streams and backpressure

**Scope:** Readable/Writable/Duplex/Transform, classic and web streams,
`stream/promises`, iterators, pipeline, buffering, encoding, destruction,
abort, and backpressure.

Use one stream state machine with adapters for classic/web protocol shapes.
Represent queued chunks and terminal events as data; derive `readableEnded`,
`writableFinished`, `destroyed`, and listener behavior. Do not create a second
stream universe for each module.

**Gate:** all 282 stream fixtures pass, including callback order, backpressure,
error/close ordering, object mode, typed-array chunks, and web adapters.

### 6. Network and protocol families

**Scope:** `net`, DNS, `dgram`, HTTP/HTTPS, HTTP/2, TLS, QUIC/DTLS, URL
request options, agents, keep-alive, framing, and socket lifecycle.

Build on one bounded socket/resource state machine and protocol adapters. Keep
parse/serialize facts pure; put polling, DNS, clocks, and OS sockets behind
capabilities. HTTP and HTTP/2 must share header/body/error facts where Node
does, while preserving protocol-specific state transitions. Test identity of
server/client/event receivers explicitly.

**Gate:** all 1,545 network fixtures pass on the declared capability profile,
with loopback/network fixtures isolated and reproducible. No debug output or
fixture-specific receiver workaround remains.

### 7. Process, child processes, workers, and clusters

**Scope:** `process`, environment/argv/signals, `child_process`, workers,
`worker_threads`, `cluster`, Atomics/shared memory, and exit semantics.

Model process/worker/child handles as resources and derive event sequences from
their state machines. Reuse the same message-port/structured-clone facts for
workers, channels, and subprocess adapters. Keep OS effects explicit and make
platform-dependent outcomes profile data.

**Gate:** all 341 process/concurrency fixtures plus process infrastructure
fixtures pass with exact exit code, signal, stdout/stderr, message ordering, and
resource cleanup.

### 8. Crypto, security, and policy

**Scope:** legacy crypto, WebCrypto, hashes/HMAC, ciphers, key objects,
randomness, TLS/X509, permissions, policy, and security validation.

Declare algorithm/key/encoding facts once; route randomness and cryptographic
providers through bounded capabilities. Unsupported algorithms must produce
the exact Node error/profile result, never a successful placeholder.

**Gate:** all 263 crypto/security fixtures and dependent TLS fixtures pass on a
documented provider/profile, with deterministic vectors and no secret leakage.

### 9. Observability and performance APIs

**Scope:** `inspector`, debugger, trace events, `v8`, heap snapshots,
`perf_hooks`, coverage, reports, diagnostics, and resource timing.

Separate observable metadata/state from optional host instrumentation. Optional
native execution consumes the same residual operations, is bounded/disposable,
and owns no alternative semantics. Generate shape/descriptor metadata while
handwriting only actual measurement and serialization algorithms.

**Gate:** all 282 observability/performance fixtures pass or are justified
profile skips; every skip names the unavailable capability and has a Node oracle
comparison.

### 10. Web and special modules

**Scope:** web globals, URL/web encoding, EventTarget, AbortController,
`fetch`, WebStreams, Blob/FormData, `readline`, `repl`, `tty`, `wasi`, `sea`,
`sqlite`, `quic`, and other modules that do not fit the core host.

Reuse the value, event, stream, filesystem, and capability facts above. A
module gets a new semantic subsystem only when Node semantics require it; a
shape-only export still needs declaration metadata and must not masquerade as
implemented behavior.

**Gate:** all 110 platform/special fixtures and the 479 cross-family fixtures
are classified and pass under explicit profiles. No “stub” is accepted merely
because a fixture did not reach an assertion.

### 11. Full-suite closure and reduction

Run the complete inventory repeatedly, compare against Node, and remove
temporary family manifests, debug probes, duplicate wrappers, and redundant
capability branches. Promote every proven fact to the declaration/IR layer;
leave only irreducible observable algorithms handwritten. Re-run size and
complexity budgets, `cargo fmt`, `git diff --check`, and the full local gates.

## Per-family evidence record

Each family change should carry one compact record (generated where practical):

```text
family, fixture, node_version, profile, oracle_result, quench_result,
observable_diff, declaration_id, capability_set, status, source_reference
```

`status` is exactly `Proven`, `Guarded`, or `Unknown`. `Unknown` behavior must
use complete slow semantics or fail visibly; it must not silently select a fast
path. A guarded optimization is admitted only after the same fixture corpus
proves that its assumptions are not observable.

## Required local workflow

For each stage:

```sh
# inspect the local Node oracle first
node tests/node/test/parallel/test-name.js

# run the smallest fixture or family through quench-node-test
cargo run -p quench-node-test --bin run -- tests/node/test/parallel/test-name.js
cargo run -p quench-node-test --bin run-parallel -- --triage --filter test-name

# verify the host and runner
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p quench-node-test --bin run-parallel -- --all --timeout-secs 30
git diff --check
```

Before choosing behavior, inspect the corresponding Node implementation/source
after observing the CLI. Commit and push one verified family stage at a time;
periodically fetch `main` and `test262`, then integrate only changes whose
semantic facts can be reconciled. If a merge conflicts in canonical runtime or
dispatch files, stop and resolve the fact model deliberately rather than
silently accepting either fork.
