# Architecture work items

This is an implementation backlog, not a status ledger. Do not add pass
counts, stage totals, completion percentages, or skip lists here. Verify work
with the relevant commands and test262 runs at execution time.

## Immediate migration gates

These gates must be applied before adding broad new runtime features. They are
architecture changes, not benchmark-dependent tuning:

- Freeze `HeapRef(u32)`, `CodeId(u32)`, `CodeRange`, `ShapeId`,
  `PropertyKeyId`, `EnvironmentRef`, and `ContinuationId` as the internal
  identity boundary. Internal arena indexes may be narrower, but the ABI is
  fixed-width.
- Hide the physical `Value` representation behind canonical value/heap
  operations. Do not make semantic code depend on NaN-boxing, enum layout,
  pointer ownership, or tag width.
- Replace direct environment-cell coupling with indexed slot load/store,
  capture, initialization, and immutability operations. Name lookup remains
  cold metadata.
- Define heap ownership, root enumeration, weak references, and realm/module/
  request/temporary/continuation lifetime domains before introducing handle
  storage or arena reset.
- Define one flat executable-code ABI and one continuation ABI before adding
  more suspension-heavy features. Runtime bodies and continuation payloads
  contain code ranges and IDs, never nested `Vec<Op>`.
- Treat `VmError` as an internal/host failure channel only. JavaScript control
  flow uses the shared Completion algebra and the single `step(machine, input)`
  entry point.
- Add the benchmark record schema and baseline workloads before accepting any
  physical encoding, cache, dispatch, paging, or superinstruction choice.

The following are explicitly deferred experiments, not competing architecture
contracts: NaN boxing versus another one-word `Value`, narrower internal
arena indexes, exact object-of-arrays layout, register-window size, cache
capacity, request-object promotion heuristics, direct-threaded dispatch,
superinstructions, moving collection, and OS page policy. None may leak into
semantic APIs or become a second execution engine.

## Machine-first acceptance contract

Every implementation item below is incomplete until it satisfies all of these
constraints:

- the hot path uses compact integers, offsets, IDs, and packed storage;
- no avoidable `String`, `Vec`, `Rc`, `RefCell`, or descriptor allocation occurs
  per ordinary property access, call, arithmetic operation, or loop iteration;
- repeated mechanics have one declaration and generated consequences;
- generic semantics remain the sole fallback for `Unknown` facts and observable
  behavior;
- a benchmark records cycles/op, branch misses, allocations, live bytes, and
  peak RSS before and after the change;
- `cargo fmt`, workspace clippy, workspace tests, and the Rust size/complexity
  lint pass;
- no JIT, native execution mode, alternate IR, shadow AST, or benchmark-only
  behavior is introduced.

The implementation order is a dependency order, not a menu. Do not add caches,
superinstructions, or other dispatch tricks to the prototype representation;
first make the representation compact, flat, and resumable.

The following are hard physical gates:

- no hot-path `Vec<Value>` resizing, per-slot `Rc<RefCell<Value>>`, trait-object
  dispatch, or string-key allocation;
- no nested `Vec<Op>` survives into executable code or continuation payloads;
- no packed fast path exists without a canonical generic fallback;
- no cache or superinstruction lands without a benchmark record;
- no heap migration starts before root enumeration and `HeapRef` lifetime rules
  are specified;
- no subsystem defines its own completion, iterator-close, or scheduling
  protocol.

## 1. Canonical semantics and completions

- Give property access, descriptors, conversion, equality, calls,
  construction, iteration, and callable checks one semantic owner each.
- Replace control signaling through incidental VM errors or nested interpreter
  results with explicit normal and abrupt completions.
- Keep protocol owners small and composable; do not build a semantic god module
  or specification DSL.
- Route every complete feature through the generic path before specializing it.

### 1.1 Universal machine and frame algebra

- Define one `Machine` state: `CodeId`, `PC`, register window, indexed
  environment, current `Completion`, and a compact tagged frame stack.
- Define one `step(machine, resume_input)` transition entry point for ordinary,
  generator, async, delegated-iterator, module, and host-job execution.
- Represent control state as data: `Try`, `Iterator`, `Await`, and `Delegate`
  frames with explicit phases and code ranges.
- Implement iterator phases as `Fetch -> Bind -> Body -> Continue`, with one
  canonical `Close` transition for abrupt completion and exhaustion.
- Implement `try` phases as `Body -> Catch -> Finally -> Resume`; finally
  completion precedence must be represented by the shared Completion algebra.
- Materialize frames only at genuine suspension boundaries. Never recover
  continuation state by scanning neighboring Ops or by adding a feature-specific
  `resume_*` walker.
- Route Promise reactions and host jobs through the same transition interface;
  use a scheduler queue only to re-enter machines, not as a second semantic
  execution model.
- Keep Rx-like push-stream abstractions out of ECMAScript iteration. Borrow
  only scheduling, cancellation, and cleanup patterns where they preserve the
  pull-based iterator protocol.

### 1.2 Physical machine contract

- Define `Machine` with fixed-width `CodeId`, `PC`, register-window base/count,
  environment reference, frame base/count, and packed completion fields.
- Define `PackedCompletion { tag, flags, payload, aux }`; map payloads to
  `HeapRef`, `ContinuationId`, `LabelId`, or immediates without allocations.
- Define frame kinds, phase widths, frame alignment, caller-save/callee-save
  registers, and the hot/cold split for frame payloads.
- Define `step(machine, input)` as the only re-entry ABI. `VmError` is reserved
  for internal/host failures, never ordinary JavaScript control flow.

## 2. Indexed heap and object representation

- Replace allocation-owning runtime values with compact immediates or
  `HeapRef(u32)` without introducing a second semantic object model.
- Move object access behind shape/slot operations; keep generic property
  semantics canonical and observable.
- Use collectible runtime strings and bounded program/realm structural-key
  tables; never use an immortal global string interner.
- Replace copied closure captures with shared indexed environments and explicit
  capture/update rules.
- Start with centralized ownership, explicit roots, generated tracing, and one
  compact non-moving collector. Add generations or movement only after a
  recorded RSS/throughput experiment proves the need.
- Specify `HeapRef(u32)` as an arena/handle index, object headers, shape epoch,
  prototype epoch, root enumeration, weak references, and realm teardown
  before converting any `Value` variant.
- Replace ordinary object/property vectors with `ShapeId + packed slots +
  PropertyKeyId`; retain a cold generic path for proxies, accessors, and
  exotic objects.
- Store ordinary bindings in dense slot arrays with initialization/immutability
  bitsets. Name hash maps are cold metadata, not the load/store representation.

## 3. Flat code and compact execution state

- Replace nested operation vectors, embedded keys, argument vectors, and copied
  function bodies with flat encoded Ops, IDs, ranges, and shared `CodeId`s.
- Use dense stack frames for ordinary calls: code ID, PC, register window,
  environment, caller position, and completion target.
- Materialize only live continuation state at genuine generator, async, job,
  module, or host suspension boundaries.
- Store frame references as compact `CodeId` plus start/end ranges and resume
  targets. A nested `Vec<Op>` is never a runtime body representation.
- Choose one physical Op encoding: fixed-width opcode words with side tables for
  uncommon operands. Do not leave “fixed-width or compact” as an unresolved
  implementation choice.
- Generate one opcode dispatch table from the Op declaration; do not layer
  recognizer matches (`simple -> control -> dispatch`) around the hot loop.
- Pre-size register windows from code metadata. Resizing register vectors is a
  representation failure, not an optimization opportunity.

## 4. Reducer contexts and operational facts

- Implement explicit `value`, `place`, `effect`, `control`, and `define`
  contexts during direct OXC traversal.
- Key `ProgramDb` facts by the relevant OXC nodes, symbols, scopes, property
  sites, and call sites without duplicating OXC-owned structure.
- Emit direct behavior for `Proven`, reusable guarded behavior for `Guarded`,
  and a cheap compact generic Op immediately for `Unknown`.
- Phase memory: parse, analyze, reduce, compact, then release OXC arenas, facts,
  source buffers, and temporary constant data unless explicitly required.
- Index facts by `NodeId/SymbolId/SiteId + ReduceContext`, not linear span scans.
  A `Guarded` fact names its guard dependencies and invalidation epochs.
- Define epochs for shape, prototype, realm, and global bindings. Observable
  mutation increments the relevant epoch and invalidates dependent rules/facts.

## 5. Declarative generation

- Introduce one macro-owned declaration for runtime values, heap layouts, and
  tracing metadata.
- Introduce one `ops!` declaration for semantic operations and mechanically
  generated physical dispatch, verification, disassembly, and profiling hooks.
- Introduce declarative builtin and primordial metadata; retain complex builtin
  algorithms as readable Rust.
- Create one canonical literal/metadata vocabulary for repeated JavaScript
  names and intrinsic facts: builtin names, property keys, prototype links,
  function names, lengths, and installation records. Generate forward lookup,
  reverse lookup, IDs, and mechanical consumers from that declaration.
- Keep internal consumers on enums or fixed-width IDs; materialize strings only
  at observable or host boundaries. Grep for duplicate literal definitions
  before adding a new spelling.
- Generate mechanical consequences only; never encode observable specification
  algorithms in a new DSL.
- Do not introduce a generic literal wrapper or duplicate semantic facts merely
  to avoid spelling a string. Coercion, ordering, reentrancy, proxy behavior,
  and abrupt-completion algorithms remain handwritten under their canonical
  protocol owners.
- One declaration must generate tags, layouts, encode/decode, tracing,
  metadata, verification, disassembly, and dispatch. Duplicate handwritten
  consequences are a design failure.
- Generated output is budgeted by source LOC, binary text, static data, and
  compile time; generation is not free merely because handwritten LOC falls.

## 6. Bounded specialization and execution performance

- Add monomorphic property, call, arithmetic, and iterator guards as generated
  `guard -> typed fast kernel -> canonical fallback` paths.
- Model each dynamic site as a bounded DLR-style rule table: guard, fast kernel,
  and one canonical generic fallback. Rules are data, never a second semantic
  implementation.
- Keep site caches fixed and small; bounded polymorphism uses a shared table,
  while megamorphic sites collapse directly to generic behavior.
- Fuse only measured frequent sequences into interpreter superinstructions under
  binary-text and RSS budgets.
- Do not add a JIT, native lowering, native code cache, or alternate execution
  backend in this scope. The compact interpreter is the only execution engine.
- A rule is `{guard dependencies, fast kernel, generic fallback}`. Guards must
  include every observable invalidation dimension; misses are bounded and then
  collapse to generic behavior.

## 7. Memory and RSS verification

- Remove avoidable `Rc`, `RefCell`, boxed trait objects, owned strings, nested
  operation vectors, string-keyed maps, and duplicated metadata from the hot
  runtime path.
- Make heap references, slots, arrays, captures, shapes, and snapshots compact
  and relocatable.
- Measure startup, one-shot, warm interpreter, hot loops, dynamic object work,
  allocation volume, retained bytes, peak RSS, binary text, static data,
  generated LOC, and handwritten LOC independently.
- Keep one interpreter policy and one residual Op vocabulary; do not multiply
  execution modes while the representation is being rebuilt.
- Every performance change emits workload, commit, cycles/op, branches/op,
  branch misses/op, allocations/op, live bytes, peak RSS, opcode text bytes,
  generated LOC, and handwritten LOC. Missing measurements block acceptance.

## 8. Engineering constraints

- Keep OXC as the only syntax and semantic frontend.
- Keep facts unified as `Proven`, `Guarded`, or `Unknown`.
- Never specialize through observable JavaScript behavior.
- Keep `quench-runtime` unaware of test262 and keep harness fidelity entirely in
  `quench-test262`.
- Preserve zero warnings, 500-line files, 40-line functions, and cognitive
  complexity ≤ 10 for every Rust change.

## 9. Test262 domain work plan

Start domain breadth only after sections 1–5 establish canonical semantics,
compact representation, flat execution, operational facts, and generation.
Implement each domain as a semantic adapter plus the smallest suitable crate
kernel. Observable Test262 behavior must execute through the canonical path and
be verified.

- **RegExp:** integrate `regress` behind `RegExpCompile`, `RegExpExec`, and
  canonical string-regexp operations. Preserve JavaScript UTF-16 indices,
  captures, named groups, flags, `lastIndex`, statics, and error ordering.
- **Date:** use `chrono` for Gregorian arithmetic and timestamp conversion;
  implement ECMAScript `TimeClip`, parsing, UTC/local conversion, legacy
  Annex B methods, invalid-date behavior, and exact object properties in the
  runtime layer.
- **ECMA-402:** select ICU4X components for `Intl.Locale`, Collator,
  NumberFormat, DateTimeFormat, DisplayNames, ListFormat, PluralRules,
  RelativeTimeFormat, Segmenter, and supported calendar/time-zone data. Use
  ICU4X data generation to minimize RSS; keep ECMA-402 option processing and
  locale negotiation in one semantic owner.
- **BigInt:** use `num-bigint` for arbitrary-precision arithmetic, with a
  narrow adapter and exact JS conversion/error semantics at the boundary. Add
  a compact small-value fast path only if measurement justifies it.
- **JSON and URI:** use `serde_json` and `urlencoding` only as internal
  algorithmic kernels after compatibility review; retain JS-specific
  traversal, ordering, Unicode, malformed-input, and exception behavior.
- **Collections and ordering:** use `indexmap` only where insertion order is
  the required storage primitive; do not delegate Map/Set identity, equality,
  iteration, or mutation semantics to the crate.
- **Stage selection:** derive runnable domain sets from the pinned test262
  directory and frontmatter. Stable `language`, `built-ins`, `annexB`, and
  `intl402` are conformance domains; `staging` is proposal work and must not
  be silently counted as stable coverage.
