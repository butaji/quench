# Wasm compatibility: one runtime, 100% spec suite

## Problem Statement

Quench needs 100% WebAssembly compatibility. Today a third-party interpreter owns compile, instantiate, invoke, and the wast harness, while the runtime never executes Wasm. That is a second VM. The destination is one runtime with three frontends — Typed TypeScript, JavaScript, and Wasm — and a spec suite that is entirely green, including in-flight proposal tests.

## Solution

One runtime. Three frontends. Wasm is the Native entry onto a Native | Fast | Dynamic ladder, not a guest engine and not a backend JavaScript compiles to.

`quench-wasm` loads: parse, validate, convert the spec operand stack to registers, emit common HIR. Third-party decode and validation are allowed. `quench-runtime` executes: specialise HIR to MIR and interpret. No JIT and no AOT in this phase. No guest interpreter in the tree.

Compatibility means passing every test in the vendored spec suite, including `proposals/`. The JS `WebAssembly` global is not part of this work. The runner scores each wast directive. Work lands as a validator harness first, then one `i32` execute slice, then spec-dependency clusters, with threads last.

## User Stories

1. As an engine author, I want a single runtime for Typed TypeScript, JavaScript, and Wasm, so that I do not maintain three VMs.
2. As an engine author, I want Wasm to enter at the Native layer, so that `i32.add` never becomes a Dynamic add.
3. As an engine author, I want JavaScript to enter at the Dynamic layer, so that JS `+` keeps coercion semantics.
4. As an engine author, I want Typed TypeScript to enter at Fast or Native, so that a proven `number + number` is `AddF64`.
5. As an engine author, I want a guard or a box to be the only way a value changes layer, so that layer crossings stay explicit.
6. As an engine author, I do not want a JSObject type or a WasmObject type, so that the object model is one ladder, not language brands.
7. As an engine author, I want common register HIR at the centre, so that every frontend emits the same shape of program.
8. As an engine author, I want the Wasm frontend to convert the operand stack to registers while it validates, so that the interpreter never simulates a Wasm stack.
9. As an engine author, I want the runtime to specialise HIR into MIR, so that execute sees only specialised operations.
10. As an engine author, I want MIR interpreted in this phase, so that spec semantics land before any compiler.
11. As an engine author, I want no JIT and no AOT in this phase, so that there is one execute consumer of MIR.
12. As an engine author, I want native kernels inside the interpreter (for example a memory fill), so that hot ops can be fast without a compiler.
13. As an engine author, I want today’s JS execute-word and instruction-stream shape not to limit this design, so that the VM can be the fastest possible for JS and Wasm.
14. As a Wasm frontend author, I want to parse and validate with a third-party library, so that I do not reimplement the spec’s binary and type rules.
15. As a Wasm frontend author, I want to translate validated Wasm into common HIR, so that the runtime never sees Wasm bytes.
16. As a runtime author, I do not want to decode Wasm, so that the shared engine stays language-agnostic at the byte level.
17. As a Wasm frontend author, I do not want to interpret, so that there is one executor.
18. As an engine author, I want the third-party interpreter removed from the tree, so that it cannot remain a fallback or a second semantics.
19. As a spec-suite owner, I want compatibility defined as the entire vendored spec suite, so that “100%” is measurable.
20. As a spec-suite owner, I want `proposals/` included (threads, custom-descriptors, wide-arithmetic), so that unfinished proposals stay on the bar and are staged last rather than skipped.
21. As a spec-suite owner, I do not want a skip list, so that green cannot mean “green except the hard files.”
22. As a spec-suite owner, I want the runner to score each wast directive, so that a 500-assertion file can be 499/500 instead of one failure.
23. As a spec-suite owner, I want `assert_malformed` and `assert_invalid` green before any execute, so that the harness is proven against the validator.
24. As a spec-suite owner, I want one `assert_return` of an `i32` export next, so that parse → HIR → interpret is proven on the smallest Native slice.
25. As a spec-suite owner, I want remaining work in spec-dependency clusters, so that GC is not attempted before tables and refs exist.
26. As a spec-suite owner, I want threads last, so that shared linear memory does not constrain the first interpreter.
27. As a spec-suite owner, I want spectest imports as Native host functions, so that the suite does not require a JS `WebAssembly` global.
28. As a Node-compat owner, I do not want this work to add `WebAssembly.Module` / `Instance` / `Memory` / `Table` to quench-node, so that Node scope stays separate.
29. As a future Node-compat owner, I want that JS API to be a later Dynamic façade over the same Native instances, so that the suite path does not wait on JS.
30. As a runtime author, I want one tracing GC for heap allocations whose lifetime is not bounded by a call, a compile, or a job, so that Native structs and Dynamic objects are not two collectors.
31. As a runtime author, I want an arena for bounded-lifetime scratch, so that parse/validate/lowering temps are not GC traffic.
32. As a runtime author, I do not want instance linear memory in the arena, so that a reset cannot free bytes a later call still loads.
33. As a runtime author, I want linear memory as a Native byte slab with instance lifetime, so that load/store/grow match the spec.
34. As a runtime author, I want tables as instance-owned Native structures whose entries are GC roots, so that `funcref` / `externref` / GC refs stay live.
35. As a runtime author, I want Native `v128` as a 16-byte slot, so that SIMD is not stuffed into an 8-byte tagged word.
36. As a runtime author, I want unboxed `i32` / `i64` / `f32` / `f64` at Native, so that numeric Wasm is raw operations.
37. As a runtime author, I want one frame walk for traps, Wasm exceptions, and Dynamic throws, so that mixed Native/Dynamic activations unwind on one stack.
38. As a runtime author, I want a trap not to match `try_table`, so that `unreachable` and OOB stay traps.
39. As a runtime author, I want a tagged `throw` / `throw_ref` to match Native `try_table`, so that Wasm 3.0 exception-handling is spec-accurate.
40. As a runtime author, I want a Dynamic throw to match Dynamic `catch`, so that JS `try/catch` is unchanged.
41. As a runtime author, I want an uncaught Native trap or Wasm exception at a Dynamic caller to become a Dynamic error, so that a later JS façade has a defined boundary.
42. As a spec-suite owner, I want `assert_trap` and `assert_exception` to stay distinct, so that the two Native failure kinds are not collapsed.
43. As a spec-suite owner, I want `assert_exhaustion` to hit a Native activation depth limit, so that infinite recursion is a spec result, not a host abort.
44. As a spec-suite owner, I want `assert_unlinkable` and `assert_uninstantiable` to run at instantiate, so that linking is part of the frontend-plus-runtime contract.
45. As a spec-suite owner, I want module registration and imports (`linking*.wast`) to instantiate multiple Native modules in one store, so that exports of one are imports of another.
46. As an engine author, I want host-call ABI shared across layers, so that spectest print and later JS imports are the same call mechanism.
47. As an engine author, I want Fast to mean guarded specialised representation (known shape, known slot, known number kind), so that JS can climb the ladder without becoming Native by wish.
48. As an engine author, I want Dynamic ops (`js.add`, `get_prop`, `call_dynamic`) to remain legal MIR, so that JavaScript is a frontend of this VM, not a second machine.
49. As an engine author, I want Native ops (`AddI32`, `WasmLoadI32`, `struct.get`, `direct_call`) to remain legal MIR, so that Wasm does not pay Dynamic dispatch.
50. As a test author, I want cluster order control → memory/tables/globals/linking → bulk+refs+tail-calls → SIMD → memory64/multi-memory/wide-arithmetic/custom-page-sizes → GC → exceptions → threads, so that each cluster has its prerequisites.
51. As a test author, I want custom-descriptors on the bar but last among proposals with threads, so that in-flight CG work does not block Wasm 3.0 core.
52. As a runtime author, I want shared linear memory and atomics only in the threads cluster, so that the memory model is extended when the suite demands it, not before.
53. As a harness author, I want wast directives parsed by a third-party wast library, so that the harness is not a second spec parser.
54. As a harness author, I want failures to name file, directive, expected, and got, so that a red cluster is debuggable.
55. As an engine author, I want compile and instantiate to be frontend-plus-runtime operations that produce a Native instance, so that invoke is just a Native call.
56. As an engine author, I want start functions to run at instantiate, so that `start.wast` is ordinary Native execution.
57. As an engine author, I want data and elem segments applied at instantiate, so that memory and tables match the spec before the first invoke.
58. As a future JS frontend author, I want this Wasm path to leave Fast and Dynamic intact, so that climbing `JsAdd` → `GuardNumber` → `AddF64` is the same specialiser Wasm already uses.

## Implementation Decisions

- One runtime, three frontends. Wasm is not a guest VM and JavaScript is not compiled to Wasm.
- Layers are Native, Fast, and Dynamic. They describe representation and dispatch, not languages. Wasm defaults to Native; JavaScript to Dynamic; Typed TypeScript to Fast or Native.
- Common HIR is a typed register program. The Wasm frontend performs stack-to-register during validation and emits HIR. The runtime specialises HIR to MIR and interprets MIR.
- Crate jobs: `quench-wasm` parses, validates, and translates Wasm to HIR. `quench-runtime` owns HIR, MIR, the interpreter, GC, arena, frames, and host-call ABI. `quench-wasm-test` discovers `.wast` files and reports directive results. `quench-node` is unchanged.
- Third-party parse, validate, and wast-directive parsing are allowed. Third-party execute is not. The current guest interpreter is removed from the tree immediately; the suite stays red until this VM grows.
- No JIT and no AOT in this phase. Interpreter-native kernels are allowed and are not a compiler.
- Existing JS execute ADRs (compact fetch stream, 8-byte tagged word) do not constrain this VM. Native `v128` is a 16-byte slot. Native scalars are unboxed.
- GC traces heap allocations at any layer whose lifetime is not bounded by a call, a compile, or a job. Arena holds only bounded scratch. Linear memory is an instance-lifetime Native byte slab, not arena and not a Dynamic object.
- Tables are instance-owned; their reference entries are GC roots.
- One unwind walk. Catch rules: traps do not match `try_table`; Wasm exceptions match `try_table` tags; Dynamic throws match Dynamic `catch`. At a layer boundary, an uncaught Native trap or Wasm exception becomes a Dynamic error.
- Spectest is a Native host module registered by the harness.
- JS `WebAssembly.*` is out of this spec. If Node later needs it, it is a Dynamic façade over the same Native instances.
- Path: (1) wast harness + validator directives, (2) one `i32` execute slice, (3) clusters in the order in user story 50, threads last.
- The runner’s pass bar is every directive in every `.wast` in the spec-suite submodule, including `proposals/`.

## Testing Decisions

- Good tests assert spec-visible behaviour: module validity, link/instantiate results, invoke return values, traps, exceptions, exhaustion. They do not assert HIR shape, register assignment, or crate internals.
- The highest seam is the spec suite as run by `quench-wasm-test`. That is the only compatibility seam. Informal probes are not the bar.
- Directive-level scoring is required. File-level pass/fail is not a sufficient report.
- Prior art: the existing `quench-wasm-test` runner and the vendored spec-suite submodule. The runner today scores whole files and delegates execute to the guest interpreter; both of those behaviours are replaced.
- Validator directives (`assert_malformed`, `assert_invalid`) are the first green evidence. Execute directives (`assert_return`, `assert_trap`, `assert_exception`, `assert_exhaustion`, `assert_unlinkable`) come after the `i32` slice exists.
- No skip list. A cluster may be red while earlier clusters are green; that is staging, not exclusion.

## Out of Scope

- JIT, AOT, and any second native semantics for MIR.
- The JS `WebAssembly` global and Node-compat coverage of it.
- Compiling JavaScript or TypeScript to Wasm.
- Replacing or reopening the JS-only stackless-VM / shape / tagged-value work except where this spec explicitly supersedes it for the shared VM.
- Writing a Quench-owned Wasm decoder or validator unless a third-party parser cannot express a required directive.
- WASI, and any host ABI beyond spectest and what the spec suite registers.

## Further Notes

Vocabulary is in `CONTEXT.md`. The decisions behind this spec are `docs/adr/0006` through `docs/adr/0014`. Those files are the paper trail; this file is the destination.

Seam: `quench-wasm-test` running the spec-suite submodule. One seam.
