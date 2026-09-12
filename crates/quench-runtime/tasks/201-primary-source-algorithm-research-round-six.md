# 201 — Primary-source algorithm research, round six

Status: complete

Research Deegen, Copy-and-Patch, V8, JavaScriptCore, SpiderMonkey, and LLVM mechanisms
that are either absent from Tasks 00–200 or materially mis-prioritized. Keep the standing
constraints: no third-party VM, no interpreter fallback, no benchmark-identity selector,
no hotness gate, rustc/LLVM cooks templates at build time, and runtime compilation is
copy plus patch plus link.

## Main conclusion

The project has not primarily run out of optimization ideas. It has implemented the
categorical composition layer before completing the executable leaves that make the
composition valuable. The default general path still enters the shared
`deegen_dyn_run` opcode loop, decodes `InlineSite` fields, switches on the opcode, and
loads/stores most values through frame memory. A copied wrapper around that loop is a
baseline-interpreter kernel, not the straight native bytecode serialization described by
Deegen, Sparkplug, JavaScriptCore Baseline, or SpiderMonkey Baseline.

This changes the order of existing work:

1. **Finish Task 36 using Tasks 149, 79, and 158 as prerequisites.** Every ordinary
   local/literal/arithmetic/compare/branch/property-hit path must be a rustc-cooked native
   stencil with burned-in operands and direct successor control. A generic executor may
   appear only on a genuine semantic slow edge.
2. **Generate a Cartesian product of useful variants, not one universal handler.**
   Deegen's baseline-JIT ablation reports a 2.69x average slowdown (maximum 5.72x) when
   specialized bytecode variants are removed, versus 1.45x average for IC removal and
   1.21x for type-based optimization removal. The exact numbers are not transferable to
   JavaScript, but the ordering is strong evidence that operand/type/location variants
   precede more hand-fused source patterns.
3. **Complete executable IC slabs and native call/return (Tasks 145, 146, 181, and 187).**
   Metadata caches that call Rust helpers do not remove the boundary. First-observation
   IC patching is allowed: it specializes semantic cases and does not gate execution on
   frequency.
4. **Carry state in registers (Task 158 and new Task 203).** Copy-and-Patch uses
   continuation parameters as a register allocation protocol, including pass-through
   values. Deegen additionally pins VM state and large boxing constants. This must be a
   typed connector context, not hidden mutable linker state.
5. **Only then add the optimizing region layer.** Reprioritize Tasks 171–177 around one
   quoted SSA/CFG value with explicit representation selection, side exits, effects,
   ownership, and liveness. Reaching 10000 from the current 1765.91 requires more than a
   conventional baseline JIT: it needs raw I32/F64 values across regions, inlining,
   check/load elimination, bounds predication, and scalar replacement.

## Newly identified gaps

- **Constructor IC and allocation/init fusion (Task 202).** Current call IC work does not
  cover `Construct`. RayTrace and Earley-Boyer contain substantial constructor traffic.
  A stable constructor case should guard the callee, allocate its known initial shape,
  initialize fields, enter an exact-arity body, and apply the JavaScript constructor
  return rule without generic `Vm::construct`/`Vm::call` boundaries.
- **Pinned tag and VM constants (Task 203).** The dynamic connector currently carries a
  frame/site pair, while tag masks and frequently used VM bases are repeatedly loaded or
  materialized. Deegen explicitly calls out tag-register optimization because 64-bit
  boxing constants cost instructions and code bytes.
- **Typed mutable binding cells (Task 204).** Object field representations are planned,
  but environment/global cells also need monotone I32/F64/tagged states so numeric regions
  can retain raw values across captured/global loads and stores.

## Refinements to existing work

- Task 162/191: include fresh-object write-barrier elision and coalescing repeated stores
  to the same receiver.
- Tasks 87/190: compile/cache the matcher independent of execution count and expose
  result-mode wrappers (`test`, capture-producing `exec`, replace/split/match) plus
  Latin-1/UTF-16 subject variants.
- Task 157: cost native instructions, helper boundaries, frame traffic, guards, and code
  bytes; never reward a larger stencil merely for reducing node count.
- Task 183: derive guarded `GetMethod + Call` recipes for array methods, Math intrinsics,
  strings, and RegExp from actual realm-local built-in identities and invalidation fuses.
- Tasks 144/171: representation selection is an explicit phase. Phi/join values choose
  I32, F64, tagged, shaped-object, or dense-array contexts; boxing occurs only at an
  effect/side exit that requires canonical state.

## Benchmark-directed diagnosis without benchmark-shaped selection

- Richards / DeltaBlue: native property-call recipes, call/constructor polymorphism,
  direct single-frame ABI, then heap allocation.
- Crypto: raw I32 SSA, literal/immediate variants, rotate/bitwise strength reduction,
  scaled dense-element addressing, and loop-wide bounds proofs.
- RayTrace: constructor IC plus polyvariant inlining, allocation sinking, raw F64 fields,
  and guarded Math kernels.
- Earley-Boyer: call ABI/inlining, constructor allocation, `instanceof` and switch
  stencils, atoms/strings, then the tracing heap.
- RegExp: native matcher kernels and result-mode/string-representation specialization.
- Splay: bump allocation, generational reclamation, known-shape allocation, and barrier
  elimination/coalescing.
- Navier-Stokes: raw F64 register residence, packed-double storage, effective-address
  stencils, bounds predication, LICM, and later vectorization.

These workloads prioritize general mechanisms; no recipe is selected from the benchmark
name, file, source location, or exact V8v7 sequence.

## Primary sources

- Deegen arXiv record (first submitted 18 November 2024):
  <https://arxiv.org/abs/2411.11469>
- Current Deegen paper and baseline-JIT ablation:
  <https://fredrikbk.com/publications/deegen.pdf>
- Copy-and-Patch variants, CPS register passing, and supernodes:
  <https://compilers.stanford.edu/publications/copy-and-patch/>
- JavaScriptCore baseline templates, ICs, watchpoints, and speculative optimization:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- JavaScriptCore FTL constructor/polyvariant results:
  <https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>
- V8 Sparkplug: <https://v8.dev/blog/sparkplug>
- V8 Maglev representation selection and SSA: <https://v8.dev/blog/maglev>
- SpiderMonkey execution tiers and CacheIR:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>
- V8 argument-adaptor removal: <https://v8.dev/blog/adaptor-frame>
- V8 object/elements representations: <https://v8.dev/blog/fast-properties>
- V8 RegExp native/bytecode execution: <https://v8.dev/blog/regexp-tier-up>
- V8 short built-in calls on AArch64: <https://v8.dev/blog/short-builtin-calls>
- V8 Liftoff register-state merging: <https://v8.dev/blog/liftoff>
