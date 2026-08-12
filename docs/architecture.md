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
  -> bounded guarded Ops and measured interpreter superinstructions
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
  dispatch, verification, disassembly, and profiling hooks;
- `builtin!` declarations generate primordial installation and callable
  metadata while readable Rust owns complex algorithms;
- specialization declarations derive `guard -> typed fast kernel -> canonical
  fallback` from canonical semantic operations;
- generate a measured, fixed interpreter superinstruction set only after a
  benchmark identifies a dispatch bottleneck and the code-size/RSS budget is
  recorded.

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

The prototype representation must be migrated behind these boundaries. New
semantic code must not couple to `Rc<Vec<(String, Value)>>`, copied closure
environments, string-keyed runtime lookup, or nested unencoded operation
vectors; those forms are temporary debt with an explicit removal order.

These boundaries are mandatory now, before adding more semantic surface:

- `HeapRef`, `CodeId`, `ShapeId`, `PropertyKeyId`, `EnvironmentRef`, and
  `ContinuationId` are stable fixed-width internal identities. Narrower
  indexes may be used inside an arena, but no variable-width reference format
  is part of the runtime ABI.
- Physical `Value` storage is hidden behind value/heap operations. The exact
  one-word encoding is a benchmarkable implementation choice; semantic code
  must not depend on its tag layout or ownership representation.
- Environment access is indexed slot load/store/capture plus cold name
  metadata. No semantic code may depend on per-slot `Rc<RefCell<Value>>`
  identity.
- Executable code and continuation payloads contain ranges and IDs only.
  Nested operation vectors are reduction-time temporaries and must not cross
  the executable-code boundary.
- Heap roots and lifetime domains (realm, module, request, temporary, and
  continuation) are specified before handle storage, arena reset, or value
  migration is introduced.

Do not commit to NaN boxing, variable-width runtime references, request-wide
heap reset, huge pages, direct-threaded dispatch, or superinstructions at this
boundary. Each remains an implementation experiment behind these contracts
and requires a before/after benchmark record.

These are physical contracts, not illustrative types:

```text
Machine    = CodeId(u32) + PC(u32) + register window + EnvironmentRef
             + FrameBase(u32) + FrameCount(u16) + PackedCompletion
OpWord     = fixed-width opcode word; uncommon operands use side tables
CodeRange  = CodeId(u32) + start(u32) + end(u32)
HeapRef    = u32 arena/handle index
PropertyKey = per-program or per-realm KeyId(u32)
```

The dispatch loop decodes one opcode tag and enters one generated handler
table. It must not walk layered recognizer matches for every operation.
Register windows are pre-sized from code metadata; ordinary execution must not
resize a `Vec<Value>` or allocate a register object.

Completion storage is packed even when its semantic API is richer:

```text
PackedCompletion = tag(u8) + flags(u8) + payload(u32) + aux(u32)
```

The payload is a `HeapRef`, `ContinuationId`, `LabelId`, or immediate according
to the tag. JavaScript control flow is never transported through incidental VM
errors.

## Universal continuation machine

All resumable execution uses one machine and one transition protocol. Do not
invent per-feature resume walkers for generators, async functions, `yield*`,
`await`, `for...of`, `try/finally`, modules, or host jobs.

```text
Machine {
  CodeId, PC, register window, Environment, Completion, FrameStack
}

Frame =
  Try { phase, handler, finalizer }
  Iterator { phase, iterator, binding, body range }
  Await { promise, resume target }
  Delegate { iterator, destination }
```

`step(machine, input)` is the only continuation entry point. A frame is
materialized only at a genuine suspension boundary; ordinary execution stays
in the compact interpreter. Frames are tagged data with explicit phases, not
Rust call-stack state and not code that searches neighboring Ops to infer where
execution stopped.

Canonical phases are mandatory for control protocols:

- iterator loops: `Fetch -> Bind -> Body -> Continue`, with `Close` on abrupt
  completion or exhaustion;
- `try`: `Body -> Catch -> Finally -> Resume`;
- await: `Evaluate -> Pending -> Fulfilled | Rejected`;
- delegation: `Open -> Resume -> Yield | Complete`.

Every transition returns the shared `Completion` algebra. Iterator closing,
finally precedence, generator resumption, promise reactions, and host jobs
must consume that algebra rather than translate through feature-specific error
conventions.

Rx-style push streams are not an execution model for JavaScript iteration.
Only its useful scheduling and cleanup ideas may inform the host/job queue;
ECMAScript iterators remain pull protocols with canonical `next`, `return`,
receiver, ordering, and `IteratorClose` semantics.

Dynamic sites use bounded rule data:

```text
Rule = guard dependencies + typed kernel + generic fallback
```

Guards record the shape, prototype, realm, and global-binding epochs they
depend on. A miss binds only within a fixed capacity; a megamorphic site
collapses to the generic protocol. No rule is a second semantic implementation.

Optimize semantic count before opcode count. Site caches start monomorphic,
remain strictly bounded, and collapse to generic lookup without growing an
optimizer subsystem. Intern structural keys per program or realm; ordinary
runtime strings remain collectible. No JIT, native lowering, or alternate
execution backend belongs to this phase.

## Implementation order

1. Canonicalize semantic protocols, the Completion algebra, and the universal
   continuation machine.
2. Replace prototype values with the indexed heap and shape/slot objects.
3. Flatten residual Ops and encode ranges, Code IDs, and compact stack frames.
4. Implement all five reducer contexts and make `ProgramDb` facts operational.
5. Generate value, heap, frame, Op, builtin, and intrinsic mechanics.
6. Add bounded monomorphic guarded Ops with canonical fallback.
7. Add only measured interpreter fusions.
8. Stop after the compact interpreter and its measured superinstructions are
   complete. JIT/native execution is a separate future scope.

Do not lead with a moving or generational collector. Begin with centralized
heap ownership, explicit roots, generated tracing, and the simplest correct
collection policy. Add regions for non-observable compiler temporaries first;
runtime regions, generations, barriers, or movement require measured evidence.

Compilation memory is phased: parse and analyze, reduce and compact, then drop
OXC arenas, semantic data, facts, source buffers, and oversized constant data
unless observable dynamic compilation or requested diagnostics require them.

The heap contract defines root enumeration from machine registers, frame
payloads, environments, job queues, realm intrinsics, and weak references
before `Value` migration begins. `HeapRef(u32)` is an arena/handle contract,
not a second object model and not an alias for `Rc` ownership.

## Performance envelope and budgets

Performance is benchmark-defined, never a slogan. Measure startup, one-shot
execution, warm interpretation, hot loops, dynamic objects, allocation-heavy
programs, and peak RSS independently. Full Test262 coverage establishes
semantics, not fast-path coverage.

Every optimization is charged for executed residual Ops, allocations, retained
bytes, bytes per live object, peak RSS, binary text, static data, generated LOC,
handwritten LOC, build time, and conformance. Use one interpreter policy without
changing Ops or semantic ownership.

Every benchmark emits a stable record containing workload, commit, cycles/op,
branches/op, branch misses/op, allocations/op, live bytes, peak RSS, opcode
text bytes, generated LOC, and handwritten LOC. No specialization or
superinstruction is accepted without such a before/after record.

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
