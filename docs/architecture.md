# Quench architecture

## One sentence

Quench is OXC program data plus unified facts plus partial evaluation plus a
small semantic algebra plus macro-generated physical specialization plus a
compact heap.

## Reduction model

```text
SOURCE
  -> OXC AST + OXC semantic data
  -> ProgramDb facts: Proven | Guarded | Unknown
  -> partial evaluator: Program + Knowledge -> Residual Program
  -> residual semantic operations
  -> physical Ops consumed by the interpreter and, later, an optional baseline JIT
```

Do not add a second syntax tree, HIR/MIR ladder, TypeGraph, self-hosted
JavaScript builtin layer, or alternate semantic pipeline. The reducer exposes
five contexts: `value`, `place`, `effect`, `control`, and `define`. The semantic
kernel stays small: load, store, property, convert, binary, compare, call,
construct, branch, allocate, suspend, and complete.

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
- specialization and superinstruction declarations derive guards and physical
  operations from canonical semantic operations.

No mechanically derivable fact should be handwritten in multiple places.

## Runtime representation target

The stable runtime boundary is:

```text
Value -> compact immediate or HeapRef(u32)
HeapRef -> heap object
heap object -> shape ID + packed slots
closure -> shared indexed environment
property site -> Cold | Mono | Poly | Generic
continuation -> code ID + PC + frame reference
```

The current prototype representation may be migrated behind these boundaries,
but new semantic code must not deepen coupling to `Rc<Vec<(String, Value)>>`,
copied closure environments, string-keyed runtime lookup, or nested unencoded
operation vectors.

Optimize semantic count, not opcode count. A small semantic algebra may have
many generated physical specializations when measurements justify them. A
baseline compiler, if added, consumes exactly the same residual Ops and owns
no alternative semantics.

## Correctness boundary

Facts may eliminate work only when they cannot suppress observable behavior.
Proxies, accessors, coercion, `Symbol.toPrimitive`, dynamic prototype changes,
direct `eval`, realms, and completion ordering remain on the generic semantic
path unless a sound guard preserves their behavior.

`quench-runtime` remains a pure JavaScript runtime. `quench-test262` owns only
test262 metadata, exact harness composition, and host classification; it may
never override harness behavior.

## Test262 domain strategy

Test262 covers ECMA-262, ECMA-402, and JSON, and its repository is organized
by domains such as `language`, `built-ins`, `intl402`, `annexB`, `harness`, and
`staging`. A domain is not a guarantee that one dependency implements the
whole domain: ECMAScript wrappers, property descriptors, coercion, errors,
identity, iteration, and observable ordering remain Quench semantics.

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

The practical order is: language/reducer primitives, ordinary built-ins,
RegExp and numeric kernels, Date, URI/JSON, then selected ECMA-402 components.
`staging` and proposal-specific tests are never treated as stable conformance
claims, while `intl402` remains a first-class ECMA-402 domain rather than a
runner exception.
