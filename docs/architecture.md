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
