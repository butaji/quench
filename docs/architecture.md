# Quench architecture

## One sentence

Quench is OXC program data plus unified facts plus direct residualization plus
one completion-aware semantic path plus generated mechanics plus a compact
indexed heap.

## Reduction model

```text
SOURCE
  -> OXC AST + OXC semantic data
  -> ProgramDb facts: Proven | Guarded | Unknown
  -> five-context reducer: Program + Knowledge -> flat residual Ops
  -> compact interpreter
  -> bounded guarded Ops and measured superinstructions
  -> optional disposable baseline-native execution
```

Do not add a second syntax tree, HIR/MIR ladder, TypeGraph, self-hosted
JavaScript builtin layer, or alternate semantic pipeline. The reducer exposes
five contexts: `value`, `place`, `effect`, `control`, and `define`. Canonical
protocol owners cover property access, conversion, comparison, call,
construction, iteration, descriptors, and completion propagation. One owner
does not mean one god module: protocols remain small and composable.

Static structure is data or disappears. VM code exists only for uncertainty.
An abstraction in the reducer does not imply a runtime allocation.

## Data-first declarations

Use declarative macros as the source of truth for mechanical runtime data:

- `value!`/`heap!` declarations generate tags, layouts, casts, tracing,
  allocation metadata, serialization, and verification;
- `ops!` declarations generate operation definitions, encoding, decoding,
  dispatch, verification, disassembly, profiling, and backend hooks;
- `builtin!` declarations generate primordial installation and callable
  metadata while readable Rust owns complex algorithms;
- specialization declarations derive `guard -> typed fast kernel -> canonical
  fallback` from canonical semantic operations;
- a measured, fixed superinstruction set may be generated only under code-size
  and RSS budgets.

No mechanically derivable fact should be handwritten in multiple places. Do
not create a DSL for observable specification algorithms; readable Rust owns
their exact coercion, reentrancy, abrupt-completion, and ordering behavior.

## Runtime representation target

The stable runtime boundary is:

```text
Value -> compact immediate or HeapRef(u32)
HeapRef -> heap object
heap object -> shape ID + packed slots
closure -> shared indexed environment
property site -> Cold | Mono | BoundedPoly | Generic
ordinary call -> compact stack frame + code ID + PC + register window
suspension -> materialized live continuation
```

The current prototype representation may be migrated behind these boundaries,
but new semantic code must not deepen coupling to `Rc<Vec<(String, Value)>>`,
copied closure environments, string-keyed runtime lookup, or nested unencoded
operation vectors.

Optimize semantic count before opcode count. Site caches start monomorphic,
remain strictly bounded, and collapse to generic lookup rather than growing an
optimizer subsystem. Intern structural keys per program or realm; ordinary
runtime strings remain collectible. A baseline compiler, if added, consumes
exactly the same residual Ops, owns no alternative semantics, uses a capped
code cache, and releases cold code.

## Implementation order

1. Canonicalize semantic protocols and explicit completions.
2. Replace prototype values with the indexed heap and shape/slot objects.
3. Flatten residual Ops and introduce compact stack frames and shared code.
4. Implement all five reducer contexts and make `ProgramDb` facts operational.
5. Generate value, heap, Op, builtin, and intrinsic mechanics.
6. Add bounded monomorphic guarded Ops with canonical fallback.
7. Add only measured interpreter fusions.
8. Add disposable baseline-native execution only when dispatch dominates.

Do not lead with a moving or generational collector. Begin with centralized
heap ownership, explicit roots, generated tracing, and the simplest correct
collection policy. Add regions for non-observable compiler temporaries first;
runtime regions, generations, barriers, or movement require measured evidence.

Compilation memory is phased: parse and analyze, reduce and compact, then drop
OXC arenas, semantic data, facts, source buffers, and oversized constant data
unless observable dynamic compilation or requested diagnostics require them.

## Performance envelope and budgets

“V8-class” is not a universal claim. Measure startup, one-shot execution, warm
interpretation, hot loops, dynamic objects, allocation-heavy programs, and
peak RSS independently. Full Test262 coverage establishes semantics, not fast
path coverage.

Every optimization is charged for executed residual Ops, allocations, retained
bytes, bytes per live object, heap RSS, cache RSS, native-code RSS, binary text,
static data, generated LOC, handwritten LOC, build time, and conformance. Use
interpreter-only, balanced, and throughput host policies without changing Ops
or semantic ownership.

## Correctness boundary

Every feature first works through the complete generic path. Facts may
eliminate work only when they cannot suppress observable behavior.
Proxies, accessors, coercion, `Symbol.toPrimitive`, dynamic prototype changes,
direct `eval`, realms, and completion ordering remain on the generic semantic
path unless a sound guard preserves their behavior.

`quench-runtime` remains a pure JavaScript runtime. `quench-test262` owns only
test262 metadata, exact harness composition, and host classification; it may
never override harness behavior.

## Test262 domain strategy

Test262 covers ECMA-262, ECMA-402, and JSON, and its repository is organized
by domains such as `language`, `built-ins`, `intl402`, `annexB`, `harness`, and
`staging`. A domain is not a guarantee that one area fully implements the whole
domain: ECMAScript wrappers, property descriptors, coercion, errors, identity,
iteration, and observable ordering remain Quench semantics.

Use mature crates for algorithmic/data-heavy kernels where their semantics
match the specification, behind the canonical runtime operations:

| Test262 area | Preferred kernel | Boundary and caveat |
|---|---|---|
| RegExp | `regress` | It targets ECMAScript syntax and supports backreferences/lookaround. The JS `RegExp` object, UTF-16 indexing, captures, flags, statics, `lastIndex`, and observable errors remain runtime-owned. Validate newer syntax and Unicode behavior against test262. |
| Date and legacy date arithmetic | `chrono` | It supplies Gregorian date/time arithmetic and timestamp conversion. ECMAScript `Date` parsing, clipping, `TimeClip`, UTC/local host policy, legacy methods, and exact observable formatting remain runtime-owned. `chrono` is not an ECMA-402 implementation. |
| Intl date/time, number, collation, locale, segmentation | ICU4X selected components | ICU4X is modular and data-driven; use generated, minimal locale/calendar data rather than linking the entire data set. ECMA-402 constructors, option coercion, supported-locale negotiation, property descriptors, and protocol behavior remain runtime-owned. |
| BigInt | `num-bigint` | It supplies arbitrary-precision digits and arithmetic. JS `BigInt` parsing, mixed-number TypeErrors, conversions, division semantics, string formatting, and object identity remain runtime-owned. |
| JSON | `serde_json` as an internal kernel where compatible | JS `JSON.parse`/`stringify` behavior, revivers/replacers, property order, numeric limits, Unicode, and exact error behavior require a semantic adapter; do not expose serde's model as a second runtime. |
| URI encoding | `urlencoding` or a narrower equivalent | Use only for the compatible percent-encoding primitive. ECMAScript URI character sets, malformed escape errors, UTF-16 treatment, and `encodeURI` versus `encodeURIComponent` remain runtime-owned. |
| Ordered keyed collections | `indexmap` where appropriate | It can provide insertion ordering, but Map/Set equality, iterator state, mutation visibility, and GC/identity remain Quench-owned. |

`regex` is not a substitute for `regress`: its linear-time engine intentionally
omits JavaScript features such as backreferences and lookaround. Likewise,
`chrono` is not a substitute for ICU4X for locale-sensitive formatting. Crates
must be selected behind semantic adapters, with feature flags and generated
data chosen for RSS and binary-size goals.

After implementation-order steps 1–5 establish the complete generic runtime,
domain breadth proceeds through language primitives, ordinary builtins, RegExp
and numeric kernels, Date, URI/JSON, then selected ECMA-402 components. Steps
6–8 optimize measured common paths and never gate semantic coverage.
`staging` and proposal-specific tests are never treated as stable conformance
claims, while `intl402` remains a first-class ECMA-402 domain rather than a
runner exception.
