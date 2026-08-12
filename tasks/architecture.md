# Architecture work items

This is an implementation backlog, not a status ledger. Do not add pass
counts, stage totals, completion percentages, or skip lists here. Verify work
with the relevant commands and test262 runs at execution time.

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
first make the representation compact and flat.

## 1. Canonical semantics and completions

- Give property access, descriptors, conversion, equality, calls,
  construction, iteration, and callable checks one semantic owner each.
- Replace control signaling through incidental VM errors or nested interpreter
  results with explicit normal and abrupt completions.
- Keep protocol owners small and composable; do not build a semantic god module
  or specification DSL.
- Route every complete feature through the generic path before specializing it.

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

## 3. Flat code and compact execution state

- Replace nested operation vectors, embedded keys, argument vectors, and copied
  function bodies with flat encoded Ops, IDs, ranges, and shared `CodeId`s.
- Use dense stack frames for ordinary calls: code ID, PC, register window,
  environment, caller position, and completion target.
- Materialize only live continuation state at genuine generator, async, job,
  module, or host suspension boundaries.

## 4. Reducer contexts and operational facts

- Implement explicit `value`, `place`, `effect`, `control`, and `define`
  contexts during direct OXC traversal.
- Key `ProgramDb` facts by the relevant OXC nodes, symbols, scopes, property
  sites, and call sites without duplicating OXC-owned structure.
- Emit direct behavior for `Proven`, reusable guarded behavior for `Guarded`,
  and a cheap compact generic Op immediately for `Unknown`.
- Phase memory: parse, analyze, reduce, compact, then release OXC arenas, facts,
  source buffers, and temporary constant data unless explicitly required.

## 5. Declarative generation

- Introduce one macro-owned declaration for runtime values, heap layouts, and
  tracing metadata.
- Introduce one `ops!` declaration for semantic operations and mechanically
  generated physical dispatch, verification, disassembly, and profiling hooks.
- Introduce declarative builtin and primordial metadata; retain complex builtin
  algorithms as readable Rust.
- Generate mechanical consequences only; never encode observable specification
  algorithms in a new DSL.

## 6. Bounded specialization and execution performance

- Add monomorphic property, call, arithmetic, and iterator guards as generated
  `guard -> typed fast kernel -> canonical fallback` paths.
- Keep site caches fixed and small; bounded polymorphism uses a shared table,
  while megamorphic sites collapse directly to generic behavior.
- Fuse only measured frequent sequences into interpreter superinstructions under
  binary-text and RSS budgets.
- Do not add a JIT, native lowering, native code cache, or alternate execution
  backend in this scope. The compact interpreter is the only execution engine.

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
