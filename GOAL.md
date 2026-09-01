# Quench VM goal

## Outcome

Build a semantically complete, fact-driven VM generator for JavaScript and
WebAssembly. The generator follows the central design of [Deegen: A JIT-Capable
VM Generator for Dynamic Languages](https://arxiv.org/html/2411.11469v2): one
operation-semantics source produces the bytecode builders, decoder, interpreter,
baseline JIT, profiling, tier transitions, inline caches, and exit machinery.

Quench is a deliberate Rust adaptation rather than a source-compatible Deegen
port. A Rust operation DSL and proc macro replace Deegen's C++ semantic API and
build-time Clang integration. `rustc` and LLVM compile the generated Rust VM and
host code. Runtime guest machine code is assembled with DynASM after a reusable
hotness decision; LLVM is never used as a runtime guest JIT.

The generator serves both frontend families through one superset schema:
JavaScript and WebAssembly declare different operation families, but share
logical values, operand sources, effects, control forms, facts, frame maps,
profiling, tiering, and backend contracts. Physical execution may specialize
those logical values: Wasm scalars and SIMD values can stay unboxed, while
JavaScript values can use its identity-preserving representation. A physical
representation is a derived view, never a second semantic owner.

The target is a two-tier engine:

```text
frontend syntax/validation
  -> generated type-safe builder
  -> canonical operation stream
  -> generated direct-threaded interpreter
       -> complete Dynamic semantics
       -> Proven/Guarded physical variants and quickening
  -> generated profiling and hotness policy
  -> runtime DynASM baseline JIT
       -> direct operation-to-machine-code lowering
       -> polymorphic ICs and cold gateways
       -> frame-mapped interpreter exits
```

Native, Fast, and Dynamic describe representation and certainty. They are not
additional semantic runtimes or JIT tiers. The baseline JIT is deliberately
simple and quick to start; an optimizing or tracing tier is outside this goal.

## What the repository implements today

The goal must be read against the current baseline, not as a progress claim.
The repository currently contains:

- OXC reduction in `crates/quench-runtime/src/reduce/` and a handwritten
  `crates/quench-runtime/src/ops_op.rs` operation enum;
- compact instruction lowering and immutable code/range storage in
  `crates/quench-runtime/src/ir.rs` and `crates/quench-runtime/src/machine.rs`;
- a Rust operation dispatcher in `crates/quench-runtime/src/vm/`, including
  compact hot helpers and complete ordinary fallbacks;
- a limited certainty model in `crates/quench-runtime/src/facts.rs`, with
  `Proven`, `Guarded`, and `Unknown` but only a small set of guards;
- a separate Wasm HIR/MIR model in `hir.rs`, `interp.rs`, and `native/`, plus
  JavaScript representations under `dynamic/` and `value.rs`.

There is currently no operation-semantics DSL, generated builder/decoder,
generated interpreter, runtime hotness/tier-up/OSR system, DynASM backend,
JIT code cache, generated frame-map machinery, or generated init/eval IC
contract. Existing compact lowering is a useful migration baseline; it is not
an implementation of the Deegen generator.

## Deegen-derived design commitments

The paper's applicable commitments are requirements, not names for existing
code:

1. **Semantics are the input.** An operation declaration contains executable
   semantic DSL forms plus the metadata needed to lower them. Syntax frontends
   do not own opcode layout or duplicate operation behavior.
2. **The builder is generated.** Frontends emit operations only through generated,
   type-safe APIs. Operand order, source kind, result arity, variants, and
   control shape are checked at generation time.
3. **The interpreter is generated.** Direct threading or the closest portable
   equivalent, register pinning, compact tags, specialization, quickening,
   monomorphic ICs, slow-path extraction, and cold-path outlining derive from
   the same operation facts.
4. **The baseline JIT is generated.** It lowers a hot operation stream directly
   to machine code without a runtime LLVM pipeline or a guest optimizing IR.
   Compilation speed is the first priority; high peak throughput is secondary.
5. **Control is explicit.** Calls, tail calls, branches, returns, throws,
   suspension, slow paths, and continuations are DSL forms. The generator must
   understand them to produce CFG metadata, call ICs, OSR points, and valid
   exits.
6. **Slow behavior remains complete.** Large, uncommon, re-entrant, allocating,
   throwing, host, proxy, and coercion paths remain ordinary gateways. Fast
   paths remove work only when admitted facts prove it safe.
7. **One IC declaration has multiple physical lowerings.** Interpreter sites
   use quickening and bounded monomorphic caches. JIT sites may use bounded
   polymorphic caches and inline slabs. Neither tier gets a separate IC
   semantics.

Deegen's Copy-and-Patch backend is intentionally replaced by DynASM. This
backend substitution must not be described as a paper result.

## Non-negotiable constraints

- **Guest JIT, not guest AOT.** JavaScript or WebAssembly guest code becomes
  executable machine code only at runtime after a reusable hotness decision.
  Build-time Rust/LLVM compilation may compile the VM, DSL expansion, semantic
  gateways, interpreter, and DynASM emitter code, but never a guest program,
  workload-specific code, or a persisted executable artifact.
- **Build with Rust and LLVM; assemble guests with DynASM.** The operation DSL
  expands into Rust and DynASM emitter templates. `rustc`/LLVM compiles the
  generated Rust and host. DynASM is the only assembler used for runtime guest
  JIT emission; no LLVM JIT runtime, guest IR compiler tier, or alternate
  semantic executor is introduced.
- **One semantic owner.** The generated operation specification is authoritative
  for opcode IDs, widths, operand sources, effects, control, facts, variants,
  fallback gateways, ICs, profiling, and physical lowerings. Raw frontend
  construction of the operation enum is forbidden after migration.
- **Universal superset, not lowest-common-denominator semantics.** JS and Wasm
  operation families may differ. Their shared schema must represent JS
  re-entrancy, coercion, proxies, allocation, and host effects as explicitly as
  it represents Wasm traps, exceptions, memory, tables, SIMD, and references.
- **Logical boxing with specialized physical representations.** The DSL has one
  logical value/boxing contract. Generated backends may unbox proven values and
  retain Wasm scalar/SIMD forms, but encode/decode and identity ownership remain
  explicit at crossings. No 8-byte Deegen layout is imposed where it cannot
  represent a required value.
- **Strict CPS/control model.** Nested executable bodies are lowered into named
  generated components and code ranges. Opaque nested Rust callbacks cannot be
  used to hide calls, continuations, suspension, exceptions, or frame effects.
- **Complete fallback first.** Every Proven/Guarded interpreter variant and JIT
  guard has a complete Dynamic or frontend-appropriate ordinary gateway. Guard
  failure, epoch invalidation, exception, proxy, callback, host edge,
  suspension, or allocation preserves observable behavior and resumes with a
  valid logical frame.
- **Explicit effects.** Allocation, coercion, prototype/shape traversal, user
  calls, exceptions, proxies, host I/O, environment mutation, scheduling, and
  suspension are operation effects. Pure fragments may be shared; effects stay
  at named gateways.
- **No benchmark footprint.** Production code and generated artifacts contain no
  benchmark names, fixture paths, source fingerprints, hashes, checksums, suite
  markers, expected scores, timing thresholds, or engine identity. Measurement
  being enabled or disabled cannot change VM semantics or tier decisions except
  through explicitly external reporting.
- **One runtime.** `quench-node` owns the Node host/API boundary;
  `quench-runtime` owns language semantics, operation generation, execution,
  allocation, and gateways. Do not add a second semantic runtime, collector,
  object model, or execution universe.
- **OXC and Wasm tooling own frontend concerns.** OXC owns JavaScript syntax.
  Wasm decoding/validation owns Wasm syntax and validation. Neither frontend
  maintains a competing operation semantics or bypasses generated builders.

## Rust operation DSL

`vm_op!` is the single declaration surface. It is intentionally an analyzable
semantic DSL, not an arbitrary Rust function body. The proc macro validates the
DSL and emits Rust modules, stable metadata, builder APIs, and DynASM templates.
Unsupported behavior must be represented by a named gateway rather than hidden
inside a macro escape hatch.

A declaration has this shape:

```rust
vm_op! {
    pub Add {
        operands: [lhs: LogicalValue, rhs: LogicalValue],
        result: [dst: LogicalValue],
        control: Next,
        effects: [Pure, MayThrow],
        facts: [Number, Type, Overflow],
        variants: [LocalLocal, LocalConst, ConstLocal],
        semantics: add_semantics {
            if both Number => return number_add(lhs, rhs);
            otherwise => gateway add_dynamic(lhs, rhs);
        },
        ic: None,
        fallback: add_dynamic,
        jit: add_dynasm,
    }
}
```

The exact syntax may evolve, but every declaration must make these facts
explicit:

- operand order, source kinds (local, constant, literal, or register range),
  logical/physical type, and result arity;
- inter-operation control shape and continuation components;
- effects, allocation/escape behavior, exception behavior, and host edges;
- required facts, guards, proven constants, and admitted variant coverage;
- complete ordinary fallback and named slow/gateway components;
- IC initialization/evaluation, epoch dependencies, and bounded capacity;
- interpreter lowering and per-ISA DynASM lowering contracts;
- profiling sites, tier-up eligibility, and OSR eligibility.

The proc macro must reject missing or extra operands, invalid types, undeclared
effects, absent fallbacks, invalid transitions, hidden control transfers, and
incomplete variant coverage. It may generate repetitive Rust and DynASM source,
but it may not execute guest code, inspect workload identity, or hide
exceptional semantics.

### Generated frontend builders

The OXC reducer and Wasm decoder use generated builders. A builder selects the
most specialized valid variant from source kinds and known facts, while keeping
frontend code independent of bytecode layout. Generated builders reject bad
argument order, missing operands, and bad operand kinds at compile time or
explicit construction validation. Direct `Op::Variant { ... }` construction is
private to generated code and focused tests; migration removes raw construction
from `reduce/*` and Wasm lowering.

### Universal logical values and physical profiles

The logical value model records tags, identity, ownership, and encode/decode
rules. Physical profiles derive storage and lowering:

- Dynamic JavaScript values preserve object/function/string/proxy identity and
  GC reachability;
- proven numeric and scalar values may use unboxed machine representations;
- Wasm i32, i64, f32, f64, v128, and reference values retain their required
  widths and trap/reference behavior;
- a compact reference to a GC value never transfers ownership to an Arena or
  native register.

`Native`, `Fast`, and `Dynamic` are derived execution views over this contract.
They do not create NativeHIR, FastHIR, DynamicHIR, JITHIR, or frontend-owned
semantic models.

## Generated execution tiers

### Interpreter

Generate a continuation/direct-threaded interpreter or the closest portable
implementation. Each operation transfers to the next operation without a
central branch forest. Generate operand decoding, register/tag handling,
local/constant variants, proven facts, cold slow paths, continuation metadata,
and complete ordinary gateways from the DSL.

The Dynamic path is authoritative. Proven paths require no guard. Guarded paths
check only their declared facts and return through the same gateway as Dynamic
on failure. The current Rust dispatcher and `CodeArena` compact executor are
migration baselines until generated equivalents pass differential tests.

### Profiling, tier-up, and OSR

Generate low-overhead per-function bytecode accounting at branches and normal or
exceptional exits. A policy-data threshold promotes future calls to the
baseline JIT. Operation declarations identify loop/back-edge sites where the
current invocation may OSR into compiled code once the function is hot.

Counters and thresholds are runtime policy data, never benchmark data. Profiling
must not alter values, descriptors, identity, ordering, errors, exceptions,
host effects, allocation semantics, scheduling, or tier-independent behavior.

### Baseline JIT

The baseline compiler consumes a hot canonical operation stream directly. It
emits DynASM instructions, burns only immutable operation metadata that is safe
to embed, uses jump-to-fallthrough layout, and splits hot code from cold
coercion, allocation, calls, exceptions, IC misses, suspension, and host paths.

Generated emitters may specialize physical representations and proven facts,
but observable behavior remains in the declared semantics and gateways. Per-ISA
capability data selects supported DynASM emitters. The first supported ISA is a
sequenced implementation choice; unsupported hosts retain the complete
generated interpreter.

### Frames, gateways, and code lifecycle

Every generated operation/control component declares live logical registers and
its box/unbox and promotion recipes. The generator emits frame maps that allow
an IC miss, guard failure, exception, host call, suspension, epoch invalidation,
or OSR exit to reconstruct the canonical interpreter frame.

JIT code owns its W^X executable pages, metadata, inline caches, and references.
Code caches are bounded, isolate-local, disposable, and invalidated by epoch or
memory policy. Invalidated code and cache state are reclaimed together. No
machine code is serialized, shipped, or reused as an AOT artifact. If allocation
or assembly is unavailable, interpretation remains complete.

## Inline caches and epochs

Cacheable operations declare the paper's two-part generic IC contract:

```text
ICSpec {
    key(input) -> ICKey,
    init(ICKey) -> ICState,       // idempotent, effect-declared
    eval(input, ICState) -> output,
    epoch_dependencies,
    capacity,
    fallback,
}
```

`init` performs the cacheable idempotent discovery; `eval` is the cheap repeated
computation. The generator lowers one declaration to tier-specific forms:

```text
Generic
  -> Mono(key, state) on successful initialization
  -> Generic on initialization miss
Mono
  -> Poly(entries) only in a capable physical tier
  -> Generic on evaluation miss or invalidation
Poly
  -> Generic on capacity, miss, or invalidation
```

Interpreter sites use monomorphic ICs and quickening. DynASM code may use a
small bounded polymorphic IC and inline slabs. Shape, prototype, realm,
global, call-target, table, memory, and other declared epochs invalidate every
dependent site. Megamorphic or unstable sites use the generic handler.

## Effects, storage, and ownership

Pure semantic fragments may be shared by generated interpreter and JIT code.
Allocation, coercion, prototype traversal, user calls, exceptions, proxies,
host I/O, environment mutation, event-loop actions, Wasm traps, and suspension
remain explicit gateways. Each gateway preserves receiver, arguments, realm,
continuation/frame state, ordering, and exception matching.

Arena storage is limited to bounded non-escaping scratch: frame headers,
register windows, PCs, continuations, immutable metadata, and proven scalar
temporaries. GC owns identity-bearing values and anything reachable from
objects, functions, strings, environments, promises, errors, proxies,
callbacks, suspended generators/async functions, host state, or Wasm tables.

```text
Arena eligibility = proven non-escape
Arena -> GC      on proven or observed escape
Arena -> reclaimed when the owning region ends without escape
GC -> GC         for identity-bearing values
```

Promotion is explicit and one-way for identity-bearing data. Logical references
in Arena registers or JIT frames do not change GC ownership.

## Reusable specialization plans

Specialized loops, packed-array operations, property walks, calls, regexp
execution, and Wasm memory/table kernels are plans derived from operation facts,
not semantic universes:

```text
KernelPlan {
    required_facts,
    logical_inputs,
    physical_storage,
    effects,
    interpreter_variant,
    dynasm_emitter,
    fallback,
}
```

Plans are admitted only by reusable facts such as counted integer loops,
F64 accumulation, packed-array get/add/set, shape-stable property walks,
known-call loops, validated Wasm bounds, or stable table entries. Every plan
retains ordinary behavior for fractions, NaN, holes, detached buffers, mutation,
prototype observability, calls, exceptions, traps, and deoptimization. No plan
recognizes source text, fixture names, scores, or engine identity.

## Quench-specific extensions

These requirements extend the paper and must be measured or explained as
Quench obligations rather than attributed to Deegen:

- one generator schema serves JavaScript and WebAssembly operation families;
- logical boxing has specialized physical profiles instead of one fixed 8-byte
  boxed register representation;
- `quench-runtime` owns a tracing GC/Arena policy and GC-visible IC/JIT state;
- `quench-node` exposes Node-compatible host effects through explicit gateways;
- JavaScript compatibility requires test262, Node APIs, descriptors, identity,
  ordering, errors, and host effects; Wasm requires validation, linking,
  instantiation, memory, tables, traps, exceptions, and host calls;
- arm64 and other ISAs require generated DynASM capability profiles;
- compilation and code-cache lifecycle must remain safe if concurrent host work
  or compilation is introduced;
- startup, steady-state, and peak RSS are optimization objectives in addition
  to throughput.

The paper's limitations remain useful scope boundaries: an optimizing or tracing
JIT, persisted code cache, and benchmark-specific compiler behavior are not
authorized by this goal.

## Anti-cheating contract

The VM, generator, and all generated production code must not contain or depend
on:

- benchmark names, fixture paths, source text, hashes, checksums, suite markers,
  expected outputs/scores, timing thresholds, or engine identity;
- dispatch keyed by workload, test runner, command line, or measurement mode;
- hidden input-shape checks whose only purpose is workload recognition;
- altered semantics, allocation, scheduling, or tier policy when measurement is
  disabled.

Benchmark harnesses may select externally named workloads and report wall time,
RSS, counters, output, and exit status. Names, scores, timings, and suite state
must not cross the production boundary. Every optimization must be explainable
as reusable semantic facts and retain a complete interpreter fallback.

## Implementation order

1. Inventory the current `ops::Op`, `ir::Instruction`, `hir::Inst`, Dynamic
   representation, Wasm representation, operand sources, effects, escapes, slow
   paths, epochs, and duplicated semantic facts.
2. Define the universal logical value, effect, control/CPS, gateway, frame-map,
   epoch, and `OperationSpec` data. Classify each fact as Proven, Guarded, or
   Unknown without changing behavior.
3. Implement the Rust semantic DSL/proc macro and compile-time validation. The
   first generated output is stable operation metadata and reviewable source.
4. Generate builders and decoders; migrate OXC reduction and Wasm lowering from
   direct enum construction. Keep the current executor as a differential
   baseline until the generated interpreter is complete.
5. Generate the direct-threaded interpreter, Dynamic gateways, physical variants,
   quickening, monomorphic ICs, tag/register metadata, and cold paths.
6. Generate function profiling, branch/exit accounting, entry tier-up, declared
   OSR sites, logical frame maps, promotion recipes, and deoptimization gateways.
7. Add direct DynASM baseline lowering from the same specs. Establish W^X,
   bounded/disposable code-cache ownership and the first ISA capability profile.
8. Add generated polymorphic ICs, inline slabs, hot/cold splitting, and
   interpreter gateways; differential-test every transition and invalidation.
9. Migrate numeric, property, call, array, regexp, JS host, Wasm memory/table,
   exception, and reusable loop plans. Remove obsolete semantic duplicates.
10. Measure externally for startup, interpreter throughput, JIT compile delay,
    steady-state throughput, compatibility, allocation, RSS, IC behavior,
    promotion, tier-up, OSR, exits, and fallback data. Optimize only reusable
    mechanisms, then apply the separate Quench performance gates.

## Tests and acceptance

- Every operation declaration expands to one opcode/spec owner with stable IDs,
  widths, sources, effects, control, fallback, and reviewable metadata.
- DSL input rejects missing/extra operands, bad types, undeclared effects,
  absent fallbacks, hidden control transfers, invalid transitions, and
  incomplete variants at generation time.
- Generated builders reject argument-order, operand-kind, result-arity, and
  control-shape mistakes; frontend code contains no raw operation construction.
- Generated interpreter, physical variants, DynASM baseline code, gateways,
  ICs, and exits are differential-tested for values, descriptors, identity,
  ordering, errors, exceptions, traps, exit status, and host effects against
  the appropriate Node/Wasm oracle.
- Function counters account for branches and normal/exceptional exits; hotness
  promotes only reusable functions; declared loop sites OSR with valid frames.
- Explicit frame maps reconstruct logical registers, receiver, realm,
  continuation, and ownership at every gateway, guard failure, invalidation,
  exception, suspension, and OSR exit.
- The logical value contract preserves JS identity/GC reachability and all Wasm
  widths, references, traps, and exceptions across physical representations.
- Arena values cannot outlive their region without promotion; GC-visible values
  remain reachable across frames, calls, callbacks, generators, async
  suspension, exceptions, ICs, and JIT code.
- Shape, prototype, realm, global, call-target, table, memory, and every other
  declared epoch invalidate dependent interpreter/JIT sites.
- JIT code obeys W^X, bounded/disposable cache policy, ABI requirements, and
  deoptimizes or exits to a valid interpreter frame. Interpreter execution is
  complete when JIT allocation is unavailable.
- JS test262 and Node compatibility suites, plus the full Wasm spec/host suites,
  compare observable behavior with their oracles. No workload fact enters
  production execution.
- External Quench benchmark runs keep `v8_v7` and Bun as a separate product
  gate: the aggregate score must eventually be strictly greater than Bun's,
  with the existing per-fixture objective retained. This gate is never met by
  fixture dispatch, source matching, score checks, or VM benchmark data.
- Memory profiles report startup, steady-state, and peak RSS, Arena high-water
  marks, GC live/retained bytes, allocation volume, and JIT/cache pages for every
  fixture. A memory improvement must come from reusable ownership/lifetime
  facts and must not regress semantics or the external performance gate.
- With measurement disabled, execution behavior and semantics are identical;
  only explicitly external reporting disappears.

## Explicit non-goals and future seam

This goal does not authorize guest AOT compilation, precompiled workload code,
persisted machine-code reuse, an LLVM runtime JIT, a guest optimizing or
tracing tier, benchmark-specific dispatch, an opaque second semantic runtime,
or a second collector/object model.

Future optimizing tiers may consume the generated operation facts, logical value
profiles, IC/epoch dependencies, profiling events, frame maps, and promotion
recipes. They must remain runtime JITs, share complete ordinary gateways, use
explicit effects, and preserve the no-cheating contract.
