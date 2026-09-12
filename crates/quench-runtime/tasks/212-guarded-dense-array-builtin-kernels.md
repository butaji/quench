# 212 — Guarded dense-array built-in kernels

Status: planned

Derive direct array built-in kernels from Task 183's canonical `BuiltinRecipe` data.
The call-site `StencilInstance` guards the actual built-in identity, receiver array
representation, prototype/species fuse, element kind, and required capacity. Its success
continuation enters one shared immutable `Kernel`; its miss continuation enters the one
generic built-in semantic edge. Property spelling and benchmark identity are never
specialization keys.

Start with one-argument `push` and zero-argument `pop`: on an established dense-array
witness they are length/capacity checks plus a direct element store/load and length
update, without `FunctionKind::Builtin` dispatch, `&[Value]` materialization, or a second
receiver lookup. Then add `slice` and `concat` as guarded bulk-copy kernels. Their fast
recipes require the standard array prototype/species facts and preserve holes and
element representation; any accessor, custom species, non-array spreadable, or
representation transition takes the generic edge.

This is not a parallel built-in implementation. The immutable recipe remains the single
semantic fact, while generic Rust execution and guarded kernel lowering are two derived
interpretations. The array witness is quoted data shared with Tasks 32 and 155. Kernel
code is shared once, and only guard constants plus continuations consume per-site memory.

The V8v7 source contains repeated `push` in Splay, DeltaBlue, RayTrace, and Earley-Boyer,
`pop` in DeltaBlue, and `slice`/`concat` in Earley-Boyer. These uses prioritize the first
recipes, but selection remains solely semantic and therefore applies to arbitrary JS.

Acceptance: identity/prototype/species/holes/capacity/element-transition tests compare
the kernel with the generic semantic implementation; disassembly proves an accepted
`push` or `pop` call has no generic built-in dispatch or argument-vector construction;
structural counters report hit/miss reasons; complete V8v7 alternating A/B improves.
All arity, capacity-growth, element-kind, and recipe-count limits are named constants.

## 2026-09-10 first slice: rejected below the call-site boundary

A minimal recipe-derived experiment attached `DenseArrayPushOne` and `DenseArrayPop`
kernel kinds to the immutable `BuiltinRecipe` catalog. It bypassed non-contiguous
argument materialization, the indirect semantic-function call, the extra receiver `Rc`
clone, and the second array check. A dedicated `CallArguments` test panicked on any
attempt to materialize and proved that both kernels instead consumed register-like
arguments directly; all 90 release tests passed.

Exact reproducible binaries were saved as `/tmp/deegen-task212-baseline`
(`f3bd4ff9...`) and `/tmp/deegen-task212-candidate` (`bedbc4b3...`). Five alternating
focused runs in `reports/task212-push-pop-focused-ab-5/comparison.txt` measured Splay
3269→3261 (-0.24%), DeltaBlue 705→702 (-0.43%), RayTrace 1633→1614 (-1.16%), and
EarleyBoyer 2282→2288 (+0.26%): aggregate 1711.89→1705.15 (-0.39%). The experiment
was therefore removed and this task remains planned.

This narrows the required granularity: a kernel entered only after `DynOp::Call` still
pays `GetStatic`/prototype lookup, canonical function-value traffic, generic callee-kind
dispatch, and the call helper boundary. The next slice must recognize the semantic
`GetStatic(receiver, method); Call(callee, receiver, args)` graph and compose the
built-in identity/receiver guards plus mutation as one call-site stencil, with the
existing generic semantic call as its miss continuation. Optimizing only the last few
instructions inside the helper is too small to matter.

Primary sources:
- SpiderMonkey documents CacheIR inlining of guarded `obj.push(value)`:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>
- V8's current append helper guards representation, grows capacity, stores directly,
  and updates length: <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/src/codegen/code-stub-assembler.cc>
- V8's current `Array.prototype.concat` retains a guarded fast path and a semantic slow
  path: <https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/src/builtins/builtins-array.cc>
