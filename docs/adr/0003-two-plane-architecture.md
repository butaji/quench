# ADR 0003: Two-plane architecture — ECMAScript execution + TypeScript semantic plane

- Status: accepted (2026-08-08)
- Context: the engine must stay 100% test262-correct (ECMAScript plane)
  while growing to TS/JSX/TSX **without erasing types**. TypeScript is
  structurally typed and intentionally unsound in places, so annotations —
  even checker-verified ones — are never runtime proof. The north-star
  design (user-supplied, 2026-08-08) keeps two parallel planes that meet
  only through **guards**, never by trusting annotations.

## Decision

### Two planes

```text
JS / TS / JSX / TSX source
          ↓
      OXC AST arena (discarded after lowering)
          ↓
 Binder + module graph + type resolution
          ↓
       Typed HIR  (rich, temporary — per function/module SCC)
   ┌──────────┴───────────┐
   ↓                      ↓
Full TypeGraph       Executable JS semantics
(persistent,         (compact bytecode —
 mmap-able metadata)   canonical exec format)
   └──────────┬───────────┘
              ↓
     Interpreter + guards
              ↓ hot
      typed specialization → baseline/JIT (later)
```

1. **ECMAScript execution plane** — always correct, dynamic,
   test262-compatible. This is the only plane that runs code.
2. **TypeScript semantic plane** — persistent, reflectable, available for
   optimization and opt-in runtime validation.

### Non-negotiable separation of three concepts

```text
TypeId   = TypeScript semantic type     (structural; many shapes satisfy it)
ShapeId  = actual runtime object layout (slots, attributes, prototype)
Rep      = machine representation       (tagged, heap ref, …)
```

Facts carry evidence: `Declared` / `Checked` / `Guarded` / `Exact`.
**Only `Guarded` and `Exact` facts may remove dynamic checks.** A checked
annotation is not runtime proof.

### Pipeline components

- **Frontend**: OXC (JS/JSX/TS/TSX, arena AST). Nothing OXC-owned is
  retained — no `&oxc_ast` references, arena-tied spans, or OXC symbols.
  Everything is copied into compact engine IDs (`NodeId`, `SymbolId`,
  `TypeId`, `FunctionId`, `AtomId` — all `u32`). The binder keeps TS's
  three mergeable namespaces: value / type / namespace-module.
- **Typed HIR**: rich and temporary (resolved symbols, CFG, narrowed
  types, effects, JSX structure, module bindings). Process one function or
  module SCC, emit bytecode + metadata, drop the HIR arena. Never the
  persistent executable format.
- **TypeGraph**: the whole TS type system as a compact persistent graph
  (`Any/Unknown/Never/Primitive/Literal/Union/Intersection/Object/Tuple/
  Function/TypeParameter/Instantiation/Conditional/Mapped/IndexedAccess/
  KeyOf/TemplateLiteral/Deferred`). `u32` TypeIds, module-local sections,
  hash-consing, lazy conditional/mapped evaluation, bounded assignability
  caches and instantiations, **no TypeId on every runtime Value**. Lives
  outside the GC heap in immutable, serializable, mmap-able module
  sections. Runtime descriptors (validator program, JSON decode schema /
  encode plan, reflection descriptor, JSX props schema) are generated
  lazily from a TypeId.
- **Checker boundary**: the VM never couples to a TS checker — it talks
  to a `TypeOracle` trait (`symbol_type`, `expression_type`, `instantiate`,
  `is_assignable`). Implementations: `SyntaxTypeOracle` (declarations only)
  → `ImportedTypeSnapshot` (differential oracle, e.g. typescript-go) →
  eventual native Rust checker. Checker compatibility is a separate large
  project; never a hard runtime dependency.
- **Executable bytecode**: compact accumulator + register hybrid (V8
  Ignition-style: u8 opcodes, variable-width operands, constant pool, atom
  table, exception table, closure layout, compressed source/type maps,
  guard table). Generic ops (`Add`, `GetProp`, `Call`, …) plus guarded
  forms (`AddI32Guard`, `GetSlotMono`, `CallKnown`, …) selected by
  feedback; guard failure falls back to the generic op **in the same
  frame** — no full deopt machinery. Bytecode is the canonical execution
  and deoptimization format; any future baseline compiler compiles
  bytecode (never OXC AST or HIR).
- **Cold/hot split**: frozen shared/mmap-able bytecode image; copy-on-write
  "quickcode" only for functions that become hot; feedback allocated
  lazily (V8-lite lesson: feedback vectors are real heap).
- **Runtime semantics**: one canonical `ecma_ops` layer (get/set/call/
  construct/to_primitive/to_property_key/abstract_equality/instance_of).
  Handlers hit fast guarded paths first and fall back to `ecma_ops` —
  never separate implementations in interpreter, builtins, and optimizer.
  (This is the same "one canonical spec-op path" as AGENTS.md point 8.)
- **Heap**: 64-bit tagged `Value`; `HeapRef(u32)` (never raw Rust pointers
  through `Value` — enables pointer compression and moving GC);
  shape-based objects (`ObjectHeader { shape: ShapeId, … }`); specialized
  array element kinds; generational GC; interned property keys;
  Latin-1/UTF-16 strings. Shapes + inline caches are the runtime
  mechanism; TS types are only semantic inputs.
- **Runtime type modes** (independent toggles): `reflection`,
  `boundary_checks`, `specialization`. Annotations are preserved and
  reflectable; validation happens at explicit typed APIs
  (`validate<T>()`, `typeOf<T>()`, `json.decode<T>()`,
  `json.encode<T>()`) and optional export/FFI/JSON boundaries.
  Check-on-every-assignment is a separate strict language mode — never
  default, since that would diverge from TypeScript behavior.
- **JSX/TSX**: kept high-level until backend selection via a `JsxBackend`
  trait (React classic/automatic, Preact, custom factory, native DOM,
  streaming SSR with a `RenderPlan` of static chunks + typed encoders).
  No React-specific semantics in the core VM.
- **Frozen module image**: `ModuleImage { atoms, symbols, types,
  functions, imports, exports, reflection?, jsx_plans?, debug? }` —
  serialize/cache after compile, mmap on next start. This is the main RSS
  architecture, not clever opcodes.

### Crate boundaries (target)

```text
frontend_oxc → compiler (binder, module graph, type graph, checker,
                          HIR, JSX, lowering)
             → bytecode (opcodes, encoder/decoder, frozen module format)
             → vm (frames, interpreter, feedback, quickening)
             → runtime (ecma_ops, realms, modules, builtins, shapes)
             → heap (Value, allocation, GC, strings)
host → vm/runtime;  jit (later) → bytecode + runtime ABI
```

`runtime`, `vm`, `heap` must never depend on OXC or the TS checker.
Cranelift (if used) must not leak into frontend/VM/runtime interfaces.
Do not turn every noun into a crate — collapse until the boundary earns it.

## Relationship to ADR 0002

ADR 0002 (compact instruction IR + pc-based interpreter, legacy AST escape
hatches) is the **near-term Tier-0 step on the ECMAScript plane** and stays
in force. Its `Instr` set is the migration path toward this ADR's compact
bytecode: same arena/ID discipline, same handler-table model, same
canonical-ops rule. What changes long-term per this ADR: instruction
encoding becomes bytecode (u8 opcodes), the TypeGraph/HIR/binder plane is
added above the compiler, and `Rc<RefCell<Object>>`-based values give way
to tagged `Value` + `HeapRef` + shapes. ADR 0002's "not bytecode yet"
applies to the current milestone only, not to the north star.

## Scope budget — ruthless 100k LOC ceiling (confidence: medium)

A ~100k LOC engine with 100% test262, JS+TS+JSX+TSX frontend,
interpreter, compact bytecode, GC, modules, async, and good performance is
**possible**. The same plus a full native TS checker, advanced optimizer,
baseline compiler, JIT, debugger/inspector, profiler, source maps, and
production tooling is **not**. Realistic allocation (reusing OXC, not
writing a parser):

| Component                       | Rust LOC |
| ------------------------------- | -------: |
| OXC integration                 |    2–4k  |
| Binder + semantic               |   8–12k  |
| TypeGraph (not full TS checker) |   8–15k  |
| HIR + lowering                  |   8–12k  |
| Bytecode                        |    3–5k  |
| Interpreter                     |   8–10k  |
| Runtime (ECMAScript ops)        |  18–25k  |
| Objects + Shapes                |    6–8k  |
| Heap + GC                       |   8–12k  |
| Builtins                        |  15–20k  |
| Host API                        |    2–4k  |

Total ≈ **85–115k LOC**. The interpreter is *not* the risk (~10k); the
LOC sinks are ECMAScript abstract ops, builtins, module loading, and
test262 edge cases. TS stays small only by reusing OXC, keeping the
TypeGraph compact, implementing needed semantics only, and never cloning
`tsc` — 100% `tsc` compatibility alone would explode into hundreds of
thousands of lines.

**Postponed (extension points designed, nothing implemented):** JIT,
baseline compiler, inspector/debugger, profiler, source-map generation,
optimizer passes, advanced TS language services, IDE APIs. A baseline
compiler arrives later as a completely separate crate over the stable
bytecode boundary.

## Consequences

- `GOAL.md`, `tasks/index.json`, `tasks/refactor-plan.md`, and
  `docs/architecture.md` describe both planes; current-stage work
  (test262 conformance, Phase A/B/C) is unchanged and stays the priority.
- The "100% test262" claim is pinned to submodule commit
  `6eec1ac9ee144dafd8f344d73a21f36bfc9f6755` with scope `test/` excluding
  `test/intl402` and `test/staging` (recorded in `tasks/index.json`);
  test262 moves continuously and TC39 states coverage is broad but not
  complete.
- R19 (bumpalo) / R20 (NaN boxing) / R21 (interning) in
  `tasks/refactor-plan.md` are reframed as steps toward the heap section
  above (tagged Value, HeapRef, shapes, interning); sequencing stays
  LATER/Phase-B.
- Performance expectations (recorded from the design review): high
  confidence on low startup/RSS with full TS metadata; medium confidence
  on beating V8 in typed web workloads (`json.decode<T>()`, typed
  serialization, streaming JSX); high confidence that an interpreter alone
  will not beat V8 on arbitrary hot JS — the baseline-compiler boundary
  stays in the architecture from day one.
