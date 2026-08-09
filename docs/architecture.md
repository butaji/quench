# Quench architecture

Quench is an OXC program reducer. It targets full ECMA-262 test262
conformance with minimal handwritten Rust, low RSS, and fast startup.

```
source -> OXC AST + OXC semantics -> ProgramDb -> reducer -> residual ops -> VM
                                      ^                |             |
                         TS/JSDoc/.d.ts/runtime facts  |       Value/Heap/
                                                        |       Continuation
                                                        +-- discard known work
```

The governing rule is: **AST is data. Types are data. Shapes are data.
Semantics are combinators. The VM exists only for uncertainty.**

## Canonical execution path

OXC owns parsing, syntax, scopes, and symbols. Quench must not create a second
syntax tree, HIR, MIR, TypeGraph, or a parallel binding representation. During
reduction it queries OXC plus `ProgramDb` facts, emits only residual execution,
then releases the OXC arena unless source tooling explicitly needs it.

The reducer exposes five contexts: `value`, `place`, `effect`, `control`, and
`define`. They compose the semantic kernel: load, store, property, convert,
binary, compare, call, construct, branch, allocate, suspend, and complete.
An abstraction is compile-time data unless runtime uncertainty requires it.
For example, lexical names become local/capture/global slots; `Place` is not a
runtime allocation.

Facts have exactly three states:

- `Proven(T)`: safe to use without a runtime check.
- `Guarded(T, Guard)`: select a specialization only after its guard succeeds.
- `Unknown`: emit generic ECMAScript semantics.

Facts can come from OXC semantics, TypeScript/JSDoc/declaration syntax,
persisted profiles, snapshots, or prior runtime observations. Their source
does not change their meaning. A fact may never remove or reorder observable
JavaScript behavior: Proxy, accessors, coercion, `Symbol.toPrimitive`, dynamic
prototype mutation, direct `eval`, realms, and completion ordering retain
generic semantics whenever they may occur.

## Residual operations and physical specialization

There is one semantic operation declaration. `ops!` generates the operation
tag, operands, compact encoding/decoder, verifier, disassembler, interpreter
dispatch metadata, counters, quickening metadata, and future baseline hooks.
Physical operations may grow from a small semantic kernel into specialized
ops and profiled superinstructions, but they never introduce independent
semantics. Generic fallback remains canonical.

Shapes and slots are the ordinary-object fast path. A site moves only from
`Cold` to bounded `Mono`, `Poly`, or `Generic`; shape-specialized operations
must guard and fall back. `HeapRef(u32)` is fundamental: heap references,
slots, captures, shapes, and snapshot data are compact indices, not host
pointers. Do not introduce `Arc<RwLock<_>>`, `Rc<RefCell<_>>`, dynamic trait
objects, or string-keyed hash maps in the hot runtime without an explicit
boundary and evidence that they are necessary.

## Declarative runtime

`quench!`, `heap!`, `completion!`, `ops!`, `builtin!`, and `specialize!` are
data declarations, not a second programming language. Each declaration owns
one repeated fact and generates mechanical consequences only: tags, layouts,
constructors, tracing, casts, serialization/snapshot metadata, descriptors,
verification, and debugging. Algorithms remain ordinary readable Rust.

Builtins are declared as runtime data: primordial graph, function identity,
name, arity, descriptors, receiver checks, and dispatch are generated.
Complex algorithms are readable Rust fast/generic handlers. Realm primordials
are generated static data, not constructed by evaluating a JS bootstrap.

`Completion` is the one control-flow algebra (`Normal`, `Return`, `Throw`,
`Break`, `Continue`); normal execution stays on the direct PC fast path.
Generators, async functions, and async generators share `Continuation { code,
pc, frame }` while retaining protocol-specific resume behavior.

## Scope and non-goals

The canonical nouns are Value, HeapRef, Shape, Slot, Frame, Code, Fact, and
Continuation. Challenge additions that create a separate subsystem universe.
The hard Rust budget is approximately 100k handwritten LOC; generated physical
complexity is acceptable, handwritten duplicate semantics are not.

No traditional AST→HIR→MIR→bytecode pipeline, self-hosted-JS builtin migration,
parallel type runtime, optimizer IR, or separate semantic JIT is planned. A
future baseline compiler may linearly consume the exact residual operations;
it is not current work and gets no alternate semantics.

`quench-runtime` owns the engine; `quench-test262` owns conformance harness
policy. The test262 stage run is the only conformance-progress authority.
