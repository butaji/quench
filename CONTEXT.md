# Quench

A JavaScript engine whose centre is one runtime with three language frontends: Typed TypeScript, JavaScript, and Wasm.

## Language

**Runtime**:
The single execution engine shared by every frontend. It owns frames, allocation, strings, GC, functions, exceptions, modules, host-call ABI, profiling, and native kernels. It is not three VMs behind a façade. It is built for speed; today’s JS interpreter shape is not a ceiling.
_Avoid_: Wasm VM, JS VM, TS VM, universal Value

**GC**:
The tracing collector for heap allocations (at any layer) whose lifetime is not bounded by a call, a compile, or a job.
_Avoid_: a second collector per language, JS object, Wasm object

**Arena**:
Bump storage for allocations that die with a bounded lifetime. Not a second GC and not a place to put anything a later call must still see.
_Avoid_: nursery (as a synonym), temporary GC

**Native**:
The layer with no remaining uncertainty: unboxed numbers, `v128`, fixed-layout structs, linear-memory bytes, direct calls. Wasm's default entry.
_Avoid_: Wasm object, unboxed (as the name of the layer)

**Fast**:
The layer that is specialized but still guarded: known shape, known slot, known number kind. Typed TypeScript's default; JavaScript after observation.
_Avoid_: JIT, IC, optimized

**Dynamic**:
The layer whose meaning is resolved at run time: JS `+`, computed property, unknown callable. JavaScript's default entry.
_Avoid_: Value, any, untyped

**Layer**:
Native, Fast, or Dynamic. A property of a representation and of an operation, not of a language. Frontends only choose the default layer; a guard or a box is how a value changes layer.
_Avoid_: JS object, Wasm object, language brand

**Trap**:
A Native instruction failure (`unreachable`, out-of-bounds load, integer divide by zero). It is not a tagged Wasm exception. The shared unwind runs; Native `try_table` does not catch it.
_Avoid_: exception, throw, error

**Wasm exception**:
A Native tagged throw (`throw`, `throw_ref`) caught by a matching `try_table`.
_Avoid_: trap, JS throw

**Frontend**:
A language-specific path that lowers source or bytecode into the common typed HIR. The three frontends are Typed TypeScript, JavaScript, and Wasm.
_Avoid_: backend, dialect, language machine

**Common HIR**:
The shared typed register program at the centre of the runtime. Frontends emit it; the runtime specialises it to MIR. The Wasm frontend converts the spec operand stack into registers while it validates.
_Avoid_: Wasm IR, JS IR, bytecode (as the shared centre), operand stack (as the execute model)

**MIR**:
The specialized executable operations the interpreter runs (`i32.add`, `js.add`, `get_prop`, Wasm load/store). In this phase it is interpreted only: no JIT, no AOT.
_Avoid_: machine code, JIT IR

**Wasm frontend**:
The validated lowering of Wasm into the common HIR. Wasm is the low-level typed subset of the runtime, not a guest of JavaScript and not a host that JavaScript compiles to. Decode and validation may be third-party; execution may not. This frontend lives in quench-wasm, not in quench-node.
_Avoid_: JS-to-Wasm, Wasm-as-backend, parser-as-VM, WebAssembly global

**Spec suite**:
The official WebAssembly specification tests vendored as a git submodule, including finalized core tests and in-flight proposal tests. Wasm compatibility means passing every test in this suite.
_Avoid_: Node WebAssembly tests, Test262, informal probes, skip list
