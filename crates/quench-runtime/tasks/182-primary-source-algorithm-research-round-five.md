# 182 — Primary-source algorithm research, round five

Status: complete

Research VM and compiler mechanisms not already represented by Tasks 00–181. Keep the
project constraints unchanged: every function executes as stencils from first use,
runtime generation is copy-and-patch only, selection is independent of benchmark
identity and hotness, and every policy limit is a named constant.

## Result

The strongest immediate finding is not a new arithmetic stencil. Native built-in method
lookup currently constructs a fresh `FunctionValue`, prototype object, and property map
for each lookup. This both allocates and destroys callable identity, so call ICs cannot
stabilize. Task 183 replaces the name-switches with one quoted built-in recipe table that
derives stable realm objects, prototype installation, generic semantics, and typed kernel
stencils.

The existing residual profile exposes two other high-confidence gaps:

- `LoadLocal,LoadName,InstanceOf,Unary:Not,JumpIfFalse` accounts for 745,752 short-run
  entries, while `instance_of` walks `Rc<RefCell>` prototype links on every execution.
  Task 185 introduces a guarded prototype-membership recipe and fused condition stencil.
- OXC switch lowering expands every case to generic strict equality plus branches.
  V8v7 contains 13 source switches, mostly in Earley-Boyer. Task 186 makes the switch a
  first-class plan with dense-integer, sparse-integer, and atom-table interpretations.

Ranked new work:

1. Task 183: canonical built-in recipe/kernel registry.
2. Task 184: liveness-driven VM frame-slot coloring.
3. Task 185: prototype-membership/`instanceof` condition stencils.
4. Task 186: first-class switch-dispatch stencils.
5. Tasks 187–189: exact-arity multi-entry calls, borrowed arguments/apply, and a
   near-code shared-kernel arena.
6. Task 190: Latin-1/UTF-16 flat, cons, slice, atom, and single-character strings.
7. Tasks 191–193: allocation folding, allocation-site policies, and composable precise
   safepoint maps after the stable heap exists.
8. Tasks 194–196: shape-owned enumeration, GVN/PRE, and bounded loop normalization.
9. Tasks 197–200: layered differential performance tests, a read-only kernel heap,
   copy-on-write array literals, and a profile-gated Swiss dictionary fallback.

Existing work absorbs three researched mechanisms instead of creating duplicates:

- build-time semantic slow-path extraction belongs to Task 149;
- effect-aware heap-load elimination belongs to Task 173;
- loop bounds predication remains Task 175, while Task 196 covers CFG-shape changes.

## Primary sources

- V8 built-ins and shared embedded code: <https://v8.dev/docs/builtin-functions> and
  <https://v8.dev/blog/embedded-builtins>
- JSC standard-library intrinsics: <https://webkit.org/blog/11934/optimizing-javascript-standard-library-functions-in-jsc/>
- LLVM stack coloring: <https://github.com/llvm/llvm-project/blob/main/llvm/lib/CodeGen/StackColoring.cpp>
- V8/JSC switch lowering: <https://chromium.googlesource.com/v8/v8/+/e0a28a6c432486017f6961bc4ba746c7b64a8a0d/src/interpreter/interpreter-generator.cc>
  and <https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/jit/JIT.cpp>
- V8 call frames: <https://v8.dev/blog/adaptor-frame>
- V8 near-code built-ins on Apple Silicon: <https://v8.dev/blog/short-builtin-calls>
- V8 strings: <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/docs/objects/strings.md>
- Allocation folding: <https://research.google/pubs/allocation-folding-based-on-dominance/>
- Allocation-site policies: <https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/43823.pdf>
- LLVM GVN/PRE: <https://www.llvm.org/docs/doxygen/NewGVN_8cpp.html>
- LLVM loop transforms: <https://www.llvm.org/docs/Passes.html>
- Layered differential performance testing: <https://arxiv.org/abs/2603.06551>

