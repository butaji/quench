# Architecture work items

This is an implementation backlog, not a status ledger. Do not add pass
counts, stage totals, completion percentages, or skip lists here. Verify work
with the relevant commands and test262 runs at execution time.

## Representation boundary

- Define the runtime heap interface around `HeapRef(u32)` without introducing a
  second semantic object model.
- Move object access behind shape/slot operations; keep generic property
  semantics canonical and observable.
- Replace copied closure captures with shared indexed environments and explicit
  capture/update rules.
- Separate immediate values, heap references, frames, continuations, and
  completion state so their storage can be packed independently.

## Declarative generation

- Introduce one macro-owned declaration for runtime values, heap layouts, and
  tracing metadata.
- Introduce one `ops!` declaration for semantic operations and mechanically
  generated physical dispatch, verification, disassembly, and profiling hooks.
- Introduce declarative builtin and primordial metadata; retain complex builtin
  algorithms as readable Rust.
- Derive specialization guards and superinstructions from canonical semantic
  operations rather than adding parallel implementations.

## Execution performance

- Encode residual Ops compactly and keep interpreter dispatch on the physical
  operation path.
- Add measured property-site specialization: cold, monomorphic, polymorphic,
  and generic.
- Fuse frequent operation sequences only through generated composition of
  primitives.
- Add a baseline compiler only after profiling demonstrates sustained hot-loop
  demand; it must consume the exact residual Ops.

## Memory and RSS

- Remove avoidable `Rc`, `RefCell`, boxed trait objects, string-keyed maps, and
  duplicated metadata from the hot runtime path.
- Make heap references, slots, arrays, captures, shapes, and snapshots compact
  and relocatable.
- Drop OXC arenas after reduction unless source-level tooling explicitly needs
  retention.
- Measure cold start, peak RSS, allocation volume, and cache-sensitive runtime
  behavior before and after each representation migration.

## Engineering constraints

- Keep OXC as the only syntax and semantic frontend.
- Keep facts unified as `Proven`, `Guarded`, or `Unknown`.
- Never specialize through observable JavaScript behavior.
- Keep `quench-runtime` unaware of test262 and keep harness fidelity entirely in
  `quench-test262`.
- Preserve zero warnings, 500-line files, 40-line functions, and cognitive
  complexity ≤ 10 for every Rust change.

## Test262 domain work plan

Implement each domain as a semantic adapter plus the smallest suitable crate
kernel. Do not mark an entire domain complete merely because the dependency
is linked; the domain is covered only when its observable test262 behavior is
implemented and verified.

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
  compact small-value fast path in the runtime representation and exact JS
  conversion/error semantics at the boundary.
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

### Dependency acceptance gate

Before adding a crate, compare its documented syntax/semantic coverage with
the relevant ECMA specification and test262 failures. Record the dependency
in `docs/DEPENDENCIES.md` in the same change, use feature flags to control
binary/RSS cost, and keep the adapter small enough to obey the Rust lint
limits. No crate may introduce a second AST, runtime object model, optimizer
IR, or alternate semantic path.

Primary references used for this plan:

- <https://github.com/tc39/test262>
- <https://docs.rs/regress>
- <https://docs.rs/chrono>
- <https://docs.rs/num-bigint>
- <https://docs.rs/serde_json>
- <https://docs.rs/urlencoding>
- <https://icu4x.unicode.org/>
