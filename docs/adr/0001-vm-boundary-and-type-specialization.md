# ADR 0001: VM boundary and type-directed specialization

## Status

Accepted (design constraints; implementation pending).

## Context

Quench targets complete test262 conformance for the pinned submodule revision:
zero failures and zero skips across script, module, async, and negative tests.
It also targets competitive
performance/RSS when it executes server-side web-framework workloads. The
project is a JavaScript VM, not a Node.js-compatible host runtime.

The test262 submodule advances only through explicit revision bumps. A bump is
accepted only when Quench again reaches the complete zero-failure, zero-skip
conformance gate.

The current TypeScript entry point parses TypeScript and lowers it to the
runtime AST after erasing TypeScript-only syntax. This cannot support using
TypeScript source annotations or declarations as compiler inputs.

## Decision

### Phase gates

1. **Phase 0 — Conformance reference.** Keep the existing evaluator and
   canonical operations as the semantic reference. The gate is the pinned
   test262 revision with zero failures and zero skips.
2. **Phase 1 — Measure and isolate boundaries.** Add only reproducible timing,
   RSS, rooted-handle, and isolate-boundary instrumentation. Keep the existing
   object representation and evaluator behavior; prove no conformance
   regression.
3. **Phase 2 — Typed IR interpreter.** Preserve externally checked type facts
   in a sidecar, lower to compact IR, and prove IR/interpreter parity against
   the reference evaluator. Type facts are guarded hints only.
4. **Phase 3 — Hot-layout experiment.** Introduce shapes/slots and dense/holey
   arrays behind the same canonical property operations. Keep dictionary mode
   as the simple fallback; proceed only when framework and VM-kernel profiles
   show the existing layout is the bottleneck.
5. **Phase 4 — Collector experiment.** Run the bounded single-isolate MMTk
   spike. Adopt a production collector only after its explicit root, weak-edge,
   host-handle, safepoint, RSS, and conformance gates pass.
6. **Phase 5 — Entry-guarded native tier.** Lower only proven hot IR functions
   to Cranelift. Guard at entry and restart in the generic IR interpreter on
   failure. Mid-function deoptimization and OSR are later phases, not part of
   the initial tier.

No later phase is required to finish an earlier one, and no phase may weaken
the pinned test262 gate.

- The VM owns ECMAScript execution, compilation, heap management, and the
  runtime/compiler interfaces needed by an embedding host. Node-compatible
  APIs, module resolution, HTTP, filesystem, timers, streams, crypto, and
  native-addon support are outside this VM's scope.
- The embedding/tooling layer supplies an already resolved graph of source
  modules and declaration files. Package resolution, project configuration,
  and filesystem access are not VM responsibilities.
- A TypeScript-compatible checker outside the VM derives type facts from that
  graph. The VM receives a versioned semantic sidecar keyed by stable module,
  symbol, function, and expression identities; it does not implement the
  TypeScript type system.
- TypeScript annotations and declaration information (`.d.ts` and equivalent
  external type facts) must survive parsing in a compiler-side type model.
  JavaScript source may receive equivalent facts from declarations or other
  configured analysis inputs.
- These facts are optimization hints, never runtime truth. Generated code must
  guard every assumption whose violation can affect observable behavior and
  transfer to a semantically complete generic path when the guard fails.
- Type facts must not alter ECMAScript parsing, scope, completion, object,
  property, proxy, or `eval` semantics. A type error is not a JavaScript
  runtime error unless ECMAScript itself requires one.
- Cranelift, inline caches, and deoptimization remain implementation choices.
  They require a stable handle/root API and reproducible workload benchmarks
  before adoption. MMTk is evaluated through a bounded single-isolate spike
  before Quench commits to a collector; the spike must prove roots, write
  barriers, host handles, a native-code safepoint path, weak edges,
  ephemerons, and ECMAScript cleanup-job ordering for `WeakRef` and
  `FinalizationRegistry`.
- Keep Quench's runtime concrete and small. Reuse mature components at clear
  boundaries, but do not introduce a generic framework for heap, collector, or
  executor strategies before a measured need exists.
- The first native tier uses entry-guarded specialization only. If an entry
  guard fails, it restarts in the generic IR interpreter before any specialized
  side effect; on-stack replacement and mid-function deoptimization are deferred
  until benchmark evidence requires them.
- Ordinary objects use immutable shapes that map property keys to compact slot
  offsets, plus a slot vector. Dynamic objects use a dictionary-mode fallback.
  Descriptor, accessor, proxy, and prototype behavior remains behind one
  canonical property-operation path. Per-isolate append-only shape storage uses
  compact integer IDs initially; no general handle crate is needed for it.
- Arrays use a dense `Vec<Value>` representation, a holey bounded-vector
  representation, and a dictionary fallback for sparse or exotic cases. Array
  length, descriptors, prototypes, and indexed-property behavior stay in the
  same canonical semantic operations as other objects.
- Performance evaluation has two tracks: a framework track selected from npm
  download data and a fixed VM-kernel track distilled from those workloads.
  Both compare Quench and V8 through the same host adapter, exclude host I/O,
  and report cold compilation, warmed throughput, p50/p99 latency, and
  steady-state RSS.
- `quench-node` is the planned Node-compatible embedding host for the framework
  track. Its current engine boundary is transitional; no Quench-versus-V8
  framework result is valid until the same `quench-node` host integration runs
  Quench.
- Quench exposes `quench-node` through a small, versioned Rust crate API:
  realm lifecycle, module registration, host calls, opaque rooted value
  handles, promise/job scheduling, and metrics hooks. The host must not depend
  on VM object layouts or borrow internal heap structures.
- Production execution is multi-isolate: each isolate has a private heap,
  collector, job queue, and owning OS thread. JavaScript values and rooted host
  handles are isolate-local. `quench-node` distributes requests and uses an
  explicit serialization/message boundary for cross-isolate communication.
- The host assigns each isolate an explicit memory budget and may recycle it at
  a safe request boundary. The VM reports per-isolate heap and RSS metrics;
  recycling must not discard live work, queued jobs, or host-rooted values.

## Consequences

- `parse_typescript` cannot remain a type-erasing-only public compiler path;
  the future pipeline needs a paired runtime AST/IR and type-fact sidecar.
- Every specialization needs a test that demonstrates fallback when a declared
  or inferred type is contradicted at runtime.
- The sidecar ABI needs source-revision validation and invalidation
  dependencies before its facts can be trusted for compilation.
- "Better than V8" remains an unmeasured target until workloads, platform,
  warmup policy, and RSS definition are specified.
- The eligibility definition and locked versions of the five-framework basket
  includes both general server frameworks and SSR/meta-frameworks. The initial
  named candidates are Hono, Express, Fastify, and Next.js; the fifth is
  selected by the locked npm-download rule and package versions are pinned for
  each benchmark run.
