# 163 — Caller-customized stencil images

Status: planned

Create bounded callee stencil images per call-site context. A context key contains the
callee identity, argument representations/shapes, receiver representation, and return
continuation context. The direct-call IC selects or creates that image on first semantic
case, and later calls jump directly to it through the VM call ABI. A named per-callee and
per-site budget bounds code growth; overflow composes the generic callee image.

This is the stencil form of Self customization, interprocedural basic-block versioning,
and SpiderMonkey Trial Inlining. It is not a hotness tier: every call executes stencil
code, and specialization is driven by the finite incoming context rather than an
execution counter. Images remain ordinary `StencilInstance` morphisms and may be shared
by reference when their structural context keys match.

The first complete recipe must fuse `GetStatic(receiver, method) + Call(arguments)` at
the caller/callee boundary. One receiver-shape guard plus Task 155's prototype/property
dependency proves the method value constant; the successful arm transfers directly to
the context-keyed callee entry and returns to the caller continuation. It must not load
the method into a `Value`, retain it, run a second callable/callee check, or cross the
Rust call dispatcher. The miss arm performs the canonical property lookup and call once.
This is one composition of existing property, dependency, and call facts, not a separate
method-call semantics.

Task 167 confirms that merely surrounding the existing Rust call kernel with a larger
stencil is not this optimization: one additional five-op direct block was genuinely
selected in Richards, yet the conditional-resume candidate regressed 0.75%. A successful
version must erase or specialize the caller/callee boundary itself.

JavaScriptCore reports that the analogous polyvariant devirtualization improved its
RayTrace workload by 38%, while polymorphic heap-access inlining improved DeltaBlue by
18%. Those figures motivate the order of work; they are not performance predictions for
this VM and do not license benchmark-specific selectors.

Acceptance: semantic tests cover recursion, polymorphic sites, missing/extra arguments,
closures, exceptions, and generic overflow; counters report image reuse and budget
fallback; a monomorphic call disassembly has no Rust `DynJitCode::call` boundary; Richards
and full-suite alternating A/B improve.

## 2026-09-10 precursor experiment: bytecode fusion is neutral

A general zero-argument static-method opcode combined the semantic
`GetStatic(receiver, key) + Call(callee, receiver, [])` pair. It preserved source
evaluation order, shared one property/call IC site, avoided the temporary callee register
and its ownership traffic, and remained selected solely from bytecode semantics. A
structural/runtime test proved the compiler emitted the coarse opcode and executed its
method exactly once; all 90 release tests and complete V8v7 smoke passed.

Instrumentation proved this was not dormant: in 100 ms windows it executed 918,018
times in Richards and 1,584,390 times in DeltaBlue. The seven-pair targeted comparison
in `reports/task163-call-static-zero-targeted-ab-7/comparison.txt` was aggregate-positive
at +1.48% but mixed. The decisive six-pair, 500 ms complete comparison in
`reports/task163-call-static-zero-full-ab-6/comparison.txt` measured 1837.24→1836.17,
or **-0.06%**. The candidate (`3a3b979d...`) was therefore rejected and removed.

This is strong granularity evidence: eliminating a whole property opcode and callee
materialization is still neutral while the combined operation calls
`Vm::call_arguments_with_ic`. The next slice must implement the acceptance criterion
literally—shape/dependency proof followed by a direct callee entry and linked return
continuation. Another semantic superinstruction that retains the Rust caller/callee
boundary should not be attempted.

Sources: <https://doi.org/10.1145/74818.74831>,
<https://arxiv.org/abs/1511.02956>, and
<https://hacks.mozilla.org/2020/11/warp-improved-js-performance-in-firefox-83/>;
JavaScriptCore polyvariant devirtualization and polymorphic inlining:
<https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>.

## Round-nineteen refinement: context-partitioned feedback planes

Attach feedback to the same structural context key as the customized image instead of
letting unrelated callers overwrite one monomorphic `InlineSite` history. The key is a
bounded product of callee identity, receiver/argument representation or shape, and return
continuation context. Equal keys share one immutable image and one compact mutable site
plane by reference. On budget exhaustion, merge contexts upward in the existing
representation/shape lattice and use its generic image; do not evict by execution count
or introduce a hotness tier.

The feedback-pollution study supplies an implementation strategy: keep context versions
ordered from most-specific to least-specific, retrieve the first compatible version, and
merge sparse facts when the bounded context budget is exhausted. Its reported reduction
in polluted compilations motivates an experiment but is not evidence of a runtime win;
this task's direct-call acceptance and full-suite A/B remain decisive:
<https://skrynski.github.io/reducingFeedbackPollution.pdf>.

## Recipe-first trial-inlining algorithm

Normalize each successful property/arithmetic/call IC arm into Task 153's quoted recipe
algebra before deciding whether to customize or inline it. The recipe contains operations
such as `GuardShape`, `LoadFixedSlot`, `GuardCallTarget`, and a result operation; mutable
shape/offset/target data lives in a compact site plane while structurally equal recipes
share cooked code. This follows CacheIR's separation of reusable code shape from per-stub
data and makes the IC result optimizer-visible instead of an opaque helper call.

When the same callee is globally polymorphic but a particular caller context is
monomorphic, attach that context's recipe plane to the `(call-site, callee)` key. Task 20
may then inline the callee using those local facts, and Task 144 propagates them through
the inlined body. Nested customization consumes `MAX_INLINE_DEPTH`; budget exhaustion
widens to the ordinary shared callee image. The trigger is availability of a semantic IC
case, never an execution-count hotness threshold.

SpiderMonkey's Trial Inlining uses distinct IC sets per call site so a globally
polymorphic function can be specialized in a locally monomorphic context, and bounds
nesting to control memory. CacheIR also shares native code for matching recipes:
<https://doi.org/10.1145/3617651.3622979>.
