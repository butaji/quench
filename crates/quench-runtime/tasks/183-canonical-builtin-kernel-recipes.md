# 183 — Canonical built-in kernel recipes

Status: in_progress

Replace `array_method`/`string_method`/`number_method` name switches and fresh native
function construction with one immutable `BuiltinRecipe` table. A recipe names the
built-in identity, installation owner/key, receiver and argument representations, result
representation, effects, generic Rust semantic function, and optional rustc-cooked fast
kernel. Rust macros derive realm prototype installation, stable `FunctionValue` identity,
generic dispatch, and stencil selection from that single fact.

A guarded call stencil verifies the actual function identity and representations, calls
the typed kernel directly, and exits to the generic semantic kernel on mismatch. Never
specialize by property spelling alone because user code may replace built-ins. Pure
numeric recipes (`sqrt`, `abs`, `floor`, `ceil`, `min`, `max`) are the smallest first
slice; array/string methods follow only when their effects are explicit.

This is the requested two-block model: recipe code is one shared immutable `Kernel`;
call-site constants and continuations are patched `StencilInstance`s. Both implement the
same connector morphism and compose with ordinary user-call regions.

Acceptance: repeated method reads have stable JS identity and allocate nothing;
`[].push === [].push` and equivalent string/number method checks pass; built-in mutation
forces the generic edge; disassembly/counters prove a numeric intrinsic avoids generic
`FunctionKind::Native` dispatch and argument materialization; full V8v7 alternating A/B
improves.

Primary sources: <https://v8.dev/docs/builtin-functions>,
<https://v8.dev/blog/embedded-builtins>, and
<https://webkit.org/blog/11934/optimizing-javascript-standard-library-functions-in-jsc/>.

## 2026-09-10 identity slice

`src/builtins.rs` now holds one macro-generated catalog for built-in identity, owner/key,
signature, effects, and Rust semantic function. `Vm` instantiates exactly one
`FunctionValue` per catalog identity for its realm. Global/Math/console/String-constructor
installation, synthesized array/string/number/RegExp method lookup, object fallbacks,
function `call`/`apply`, and generic invocation all derive from those identities. The
uncached `Vm::native` constructor remains only for dynamically supplied host callbacks.

This slice removes fresh `FunctionValue`, prototype, property-map, and JIT-cell
allocation from every synthesized method read while retaining distinct JavaScript
identities for built-ins that share a Rust semantic function. Prototype installation,
mutation-visible lookup, guarded numeric kernels, and A/B evidence remain before the task
can complete.

## Identity-slice measurement

The complete five-repetition alternating comparison is recorded in
`reports/task183-builtin-identity-full-ab-5/comparison.txt`. The candidate geometric
mean improved from 1669.27 to 1765.91 (+5.79%). Richards, DeltaBlue, and Crypto were
slightly negative, while RayTrace (+9.21%), Earley-Boyer (+27.37%), and RegExp
(+16.89%) improved. Retain the canonical-identity slice, but do not call this task
complete: the source still synthesizes method lookup instead of installing and observing
the actual realm prototypes, and no built-in hit is yet an inline native stencil.

Full-suite verification after the first Task 202 slice exposed that
`BUILTIN_RECIPES` was a Rust `const` slice. A const reference may be separately promoted
at each use, contradicting the intended canonical-address invariant and making the
identity-law test depend on compiler promotion choices. It is now a process-wide
immutable `static` table, matching the shared-kernel model and making
`BuiltinId::recipe()` return one stable recipe address.

Task 212 is the derived dense-array lowering of this catalog. `push`, `pop`, `slice`,
and `concat` remain facts here exactly once; Task 212 may add guarded immutable kernels
and patched instances, but must not introduce a second identity, signature, effects, or
generic-semantics table.

## Round-seventeen typed reducer scope

Use V8's current builtin reducer as the concrete first catalog boundary. Guarded
`sqrt`, `abs`, `floor`, `ceil`, `round`, `min`, and `max` become representation-specific
numeric morphisms; IEEE-754 `pow`, `log`, and trigonometric calls become shared immutable
kernels with typed inputs and outputs. The call-site stencil guards builtin identity and
argument representation, then enters that morphism without constructing a generic
native-call argument slice. Missing arguments, coercion-capable inputs, overwritten
builtins, and observable effects retain the generic recipe edge.

Do not assume that a Rust `f64` method became a hardware instruction: disassemble every
template and distinguish actual LLVM intrinsics from libcalls. V8's maintained reducer
vocabulary is primary evidence for semantic cases, not permission to copy V8 source or
introduce runtime compilation:
<https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/maglev/maglev-reducer-inl.h>.
