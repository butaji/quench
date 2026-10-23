# Quench JIT design

## Goal

Make `quench-node` score at least 20% above every measured peer on JetStream 3.0 in both no-JIT and JIT modes, while using less maximum RSS in JIT mode. These are separate performance goals: JIT tier work cannot improve no-JIT execution because no-JIT never enters compiled tiers. Keep the interpreted mode as the default and preserve its results. Use one OXC frontend, one residual bytecode representation and the same `Vm`, heap and host APIs in both modes. TypeScript is erased by the existing frontend; Wasm keeps its current decoder and enters that same VM.

The pass condition is measured on stock JetStream with 11 or more matched rounds: in each execution mode, the lower bound of the paired 95% bootstrap interval for Quench's suite score ratio must be at least 1.20 against every available peer. In JIT mode, the paired maximum-RSS ratio must also remain below 1 against each peer. Qualify on Linux x86_64 and M4 separately. A single run is a screen, not a win. JetStream 3 contains both compute-heavy JavaScript and Wasm, so continue to its Wasm fixtures after the JavaScript tier has a useful measured path.

Track no-JIT independently through interpreter, allocation, garbage collection and host-call paths; compiled-tier coverage only contributes to the JIT objective. Existing historical screens are far from the new threshold (about 0.49× Bun's suite score in no-JIT and 0.058–0.085× the JIT peers in JIT), so architecture consolidation is enabling work rather than evidence of a near-term suite win. Rebaseline both modes on this branch before attributing any change, and rank follow-up work by hot execution coverage and paired score deltas.

## Unified execution architecture (active contract)

There is one JavaScript bytecode semantics authority. A Rust macro declaratively records each opcode's operands, effect class, lane behavior and compiled denotation; it generates the fast-lane handler wiring and the per-instruction lowering entry points in `vm/compiled.rs`. Both generated paths call the same semantic helpers for observable behavior. A new opcode implementation therefore has one semantic definition, and an optimization may specialize only behind explicit guards that retain the same helper or resume the lane on failure. This is the contract for replacing duplicated hand-maintained lane and compiler decision trees.

`vm/compiled.rs` is the sole region compiler. A region mode distinguishes function entry from loop OSR; both use the same bytecode decoder, instruction denotations, abstract state, guard representation, code-memory publisher and resume-map format. The OSR entry supplies the current loop-header PC and live register state. Any failed speculative guard returns to the fast lane at that exact failing instruction with the interpreter-visible frame, registers, handlers and pending completion intact. Effects remain ordered through shared semantic helpers. No failure path restarts a region after effects or skips an instruction.

Every native artifact, Wasm function or JS loop region, has one entry: the `Entry` ABI (call context, base, length, arguments, result word) -> status, entered through `enter_native`. Where the host resumes faults (x86-64 Linux), that runs the trampoline, and backward edges read the VM's poll page, which faults once termination is requested and returns the terminated status. Elsewhere, code checks the context's interrupt flag. A JS loop region stopped this way reports `LoopOutcome::Terminated`, and the lane leaves through the backward jump's safepoint. No guest code runs on the registers of the unfinished iteration.

One tier manager owns hotness counters, promotion decisions, compiled artifacts, cache lookup, byte accounting and exhaustion behavior for JS and Wasm on the existing `Vm`. Frontends continue to lower into the same residual bytecode and do not create frontend-specific VMs. Tier admission depends on measured execution feedback and named resource budgets, never benchmark identity, source markers, score, checksum or suite selection. The cache key names the bytecode artifact and region mode; cache eviction or exhaustion returns execution to the existing lane/interpreter.

Migration order is: make the opcode semantics macro generate the lane and compiled denotation views; converge function and OSR compilation behind one region-mode API in `compiled.rs`; add general guarded field, dense-element, inline-cache call and int32/f64 denotations before deleting their equivalent `js_compiled.rs` recognizers; then add a rooted VM continuation for effectful calls. The existing `compiled/osr_loop.rs` remains a cohesive OSR analyzer/lowerer behind the shared compiler API. Each implementation step is separate, with runtime library tests and the 116 Node-oracle scripts run in interpreter and `--jit` modes. At measured milestones, `QUENCH_JS_JIT_STATS` must show native entries on hot V8-v7 code covering Richards, DeltaBlue, NavierStokes and Crypto; JetStream Score/RSS comparisons are reported against JIT-enabled Node and Bun without treating a diagnostic run as a win.

## What the repository already has

At the `v2-merged` baseline, `quench-runtime` already used Cranelift to compile a subset of Wasm functions through `ExecutionMode::Compiled`; JavaScript and TypeScript still used the residual bytecode interpreter, and `quench-node` had no process-level JIT selector. This branch adds `--jit` to the existing runtime path and a lazy JS function-entry tier. The Wasm compiler remains a reference for executable memory, traps and host callouts.

The first memory risk is the granularity of code ownership: compiling all eligible functions when one Wasm function is first called makes cold code consume both compiler work and executable bytes. The JavaScript tier should compile a function only after it is hot, account for emitted bytes, and stop compiling when a named per-process budget is full. Keep no IR, AST or copied bytecode alongside native code unless a correctness invariant needs it. Code-cache exhaustion means interpreted execution.

JavaScript and Wasm share one JIT runtime. Their semantic lowerers are necessarily different because JS values and effects differ from Wasm's fixed scalar types, traps, and linear memory; their target-ISA construction, executable-memory allocation, tier budgets, diagnostics, and entry/cache management remain common. `vm/jit_backend.rs` caches Cranelift target ISAs by optimization and verifier settings for both lowerers, and both publish through the same W^X `CodeMemory`. `ProgramEntry` holds one typed sparse `JitTierManager` table for both frontends, eliminating the unused second hash-map header on every program. The manager now owns both the JS statistical promotion transition and Wasm call-hotness transition; `ProgramStore` applies global function counts, shared code-cache accounting, and diagnostic side effects. The admission criteria and artifact ceilings remain frontend-specific inside the shared manager and aggregate `JitCodeCacheBudget`, respectively. JS function compilation and OSR loop lowering still use different semantic lowerers, and the effectful JS call continuation is not implemented yet. Separate bytecode semantics do not justify separate JIT runtimes.

## Research and the design it supports

Current engines use tiers because compilation speed, warm-up cost, code quality and code size pull in different directions. V8's Sparkplug emits a baseline from bytecode, Maglev is a faster mid-tier, and its top tier spends more work on hot code; V8 also scales interrupt budgets using invocation count and bytecode length. JavaScriptCore keeps bytecode as the source of truth and tracks execution counts, including loop backedges, before tier-up. These mechanisms support size-aware, workload-sensitive admission, but their numeric thresholds should not be copied into Quench. The inference for Quench is to add one small native tier first and measure its compile cost and code bytes before adding a second optimizing tier. [V8 Maglev](https://v8.dev/blog/maglev), [V8 tiering manager](https://chromium.googlesource.com/v8/v8/+/HEAD/src/execution/tiering-manager.cc), [JavaScriptCore speculation and tier-up](https://webkit.org/blog/10308/speculation-in-javascriptcore/), [JavaScriptCore bytecode format](https://webkit.org/blog/9329/a-new-bytecode-format-for-javascriptcore/), [SpiderMonkey optimization tiers](https://d2r03xnqvbcxrv.cloudfront.net/js/how-we-optimize.html)

For Wasm, V8's Liftoff and JavaScriptCore's BBQ show the same split more clearly: a fast baseline compiler generates code on demand, while TurboFan or OMG recompiles selected hot functions with more expensive optimization. V8 describes Liftoff as a one-pass compiler and delays top-tier work until functions heat up; WebKit reports BBQ compiles around four times faster than OMG while producing code roughly twice as slow. These are design points, not target ratios for Quench. Quench currently compiles its supported Wasm functions together when first entering the module, so a future lazy per-function tier can reduce work on cold code; the earlier root-closure screen did not improve RSS or score, so that direction still needs stronger admission and code-lifetime evidence. [V8 Wasm compilation pipeline](https://v8.dev/docs/wasm-compilation-pipeline), [WebKit BBQ and OMG](https://webkit.org/blog/7691/webassembly/)

Futamura's first projection specializes an interpreter for a fixed program. Quench already has the useful static input: validated residual bytecode with fixed opcodes, operands, branch layout, constants and function boundaries. Lower that residual form to Cranelift, so native execution removes opcode dispatch, operand decoding and the generic register file where the function proves those facts. This is the practical projection; a general self-applicable partial evaluator or compiler generator is outside the goal. Dynamic JavaScript values remain dynamic: emit a guard only when it proves a fast representation, and use the same semantic slow path or resume the interpreter when it fails. [Futamura's paper](https://www.jstage.jst.go.jp/article/jssst/21/5/21_5_343/_article/-char/en), [Ershov on mixed computation](https://doi.org/10.1016/0304-3975(82)90111-6), [practical partial evaluation for dynamic-language runtimes](https://dl.acm.org/doi/10.1145/3062341.3062381)

Ershov/Sethi-Ullman numbering estimates the register demand of expression trees and schedules the higher-demand child first. Use it to order tree-local evaluation or short straight-line expression regions when the values are unboxed. Do not use the tree number as a whole-function allocation strategy: JavaScript control flow has joins, loops, calls and deoptimization edges. Lower those to Cranelift's SSA graph and let its allocator handle live ranges and spills. [Optimal code generation for expression trees](https://doi.org/10.1145/800116.803770)

Use category theory as a correctness discipline for the one-VM architecture. Validated bytecode blocks and Cranelift blocks are composable state transitions; interpreter and native lowering interpret the same opcode/effect algebra. For each supported transition, preserve a relation between interpreter and native states, then use identity and composition to lift local preservation to a whole acyclic function. In the string-return tier the relation maps guarded parameter registers to the same `f64` inputs and abstract string registers to rooted constant indices. `NumberRelation` now classifies Number comparisons once for interpreter and native lowering, while Cranelift supplies the target predicate. A branch preserves the relation and chooses the same successor; a leaf returns a bytecode PC, and the ordinary VM loads the rooted constant and performs the regular Return/unwind. The compiler's forward-only, no-join restriction gives a topological induction proof. Exceptions, heap writes, host calls, and collection safepoints remain explicit effects and are outside this tier's accepted subset. This is a proof target and review method, not a runtime abstraction or a claim that category theory itself improves speed. It lets OXC/TypeScript residualization and Wasm decoding compose with the shared VM without separate frontend VMs. See [compositional compiler correctness](https://doi.org/10.1145/3729276) and [correctness of speculative optimization and deoptimization](https://arxiv.org/abs/1711.03050).

Deegen supplies a useful meta-compiler pattern: write bytecode semantics once, generate interpreter and baseline-JIT implementations, and assemble fast baseline code from Copy-and-Patch stencils without running LLVM in the VM. Reuse the semantic single-source idea and the low runtime compilation cost. Keep Cranelift for the optimizing tier where CFG/SSA optimization pays off. Deegen's reported results are for Lua, and its paper leaves an optimizing tier outside its scope; they do not establish JavaScript performance or lower RSS for Quench. Treat stencils as a future baseline path only after opcode semantics have a shared definition and measured runtime compilation is the bottleneck. [Deegen paper](https://arxiv.org/abs/2411.11469), [Stanford Deegen project](https://compilers.stanford.edu/software/deegen/), [Copy-and-Patch](https://doi.org/10.1145/3485513)

Use online statistics for promotion and acceptance instead of a fixed “hot” intuition. A candidate is worthwhile when its estimated future execution savings exceed compile cost plus its code-memory charge: `E[remaining runs] × E[time saved per run] > compile time + λ × native bytes`. Type stability is only evidence for a guard, not evidence of payback; calibrate compile cost, per-call savings, and future reuse before adding an economic promotion gate. Apply an explicit process-wide code-byte/page budget and discard low-value code when full. For benchmark decisions, pair engine runs by fixture and round, use randomized or balanced engine order, compare log score ratios and RSS ratios, and bootstrap the paired rounds; do not infer a win from unpaired averages or one run. Size confirmatory samples from paired-log-ratio variance and a predeclared minimum effect, because more bootstrap replicates do not create more process samples. Keep cold-start and warmed paths separately visible because compilation can raise both elapsed time and RSS before it pays back. These policies are Quench design inferences, not guarantees from the cited papers.

Function-entry counts miss a long loop that is called only once. If diagnostic evidence identifies that profile, add backedge hotness and an on-stack replacement entry at a loop header, with an explicit register/PC map so the same activation can move into compiled code. V8's interrupt budget and OSR path, and Deegen's selected-bytecode OSR, provide precedents; they do not justify adding loop compilation before a workload demonstrates the need. Keep baseline code and optimizing code under one memory budget, and retire the baseline mapping when replacement is safe. A Copy-and-Patch baseline is a later candidate only if measured Cranelift compile cost or temporary RSS limits payback; Sethi-Ullman scheduling belongs in that baseline only for pure expression trees, since Cranelift already owns SSA register allocation. [V8 tiering and interrupt budgets](https://github.com/v8/v8/blob/main/docs/runtime/tiering.md), [V8 OSR path](https://github.com/v8/v8/blob/main/src/execution/tiering-manager.cc), [Deegen paper](https://arxiv.org/abs/2411.11469), [Copy-and-Patch](https://doi.org/10.1145/3485513), [Sethi and Ullman](https://doi.org/10.1145/321607.321620).

JetStream 3 is the acceptance workload, not a code-generation oracle. Its stock driver and per-test validation remain unchanged. The repo's runner already provides JetStream's shell surface through `node:vm`, fresh realms, on-demand resource loading and process-level maximum RSS, so benchmark changes should be limited to selecting the Quench execution mode and recording its identity. [JetStream 3 announcement](https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/), [pinned JetStream corpus](https://github.com/WebKit/JetStream/tree/JetStream3.0)

## Implementation sequence

1. Record the mode-specific JetStream control runs and compiler/runtime versions. Keep each fixture separate and start with the first server-side fixture in `docs/jetstream3.md`.
2. Add a process-level JIT selector to `quench-node`, defaulting to interpreted. It must set `ExecutionMode` on the existing runtime rather than create a JIT-specific VM. Confirm the Wasm tier remains reachable through JavaScript `WebAssembly` calls in the same runtime.
3. Add lazy JavaScript function entry counters and a budgeted compiled-code cache. Avoid per-op profiling metadata; retain only feedback needed to justify a compile decision. Make compile failure, unsupported bytecode, guard misses and budget exhaustion resume through the interpreter.
4. Compile a narrow, semantically safe JavaScript subset first. Start with pure numeric functions and entry guards checked after normal activation. The tier must return through the existing dispatch-loop unwind path. Extend to loops/control flow only with PC/state maps that permit exact interpreter reconstruction. Keep effects behind semantic helpers until ordering, roots and exception behavior are covered by the semantic-preservation relation.
5. Use JetStream's server-side fixtures one at a time. Profile score and peak RSS together, including compilation and executable memory. Keep a tier only when paired confidence intervals clear both score and RSS bars. After JavaScript, measure the Wasm-tagged fixtures with compiled mode and reduce cold code retention where RSS warrants it.
6. Qualify retained changes with at least 11 matched rounds on Linux and M4. Store raw reports and reproduction commands under `evidence/`; update the Linear scoreboard at its existing home. Do not claim a win from an unsupported competitor, a different host, or a one-round screen.

## First JS tier in this branch

`quench-node --jit` selects `ExecutionMode::Compiled` on the same runtime and VM; the default remains interpreted. The first JS tier is intentionally narrow. Its profile is a Bernoulli observation per eligible entry: success means every formal argument is a Number. For each function, it tests at four prespecified looks (32, 64, 128, 255), with critical success counts (31, 59, 113, 219). Each exact binomial upper-tail test spends at most `0.05 / 4` under the null that Number-entry probability is at most 0.80, so a union bound limits false promotion across repeated looks for one function to 5%, assuming stationary independent entries. The 256-function tracking cap does not turn this into a 5% VM-wide family-wise guarantee; controlling that would require allocating error across candidates or a separately calibrated online procedure. A successful look marks promotion; if that entry has a non-Number argument, promotion stays pending until the next Number entry can compile and enter natively. A failed final look leaves the function interpreted. This improves the former repeatedly-peeked Wilson threshold, which did not retain its nominal fixed-sample coverage after optional stopping. If workload phase changes violate the stationary model, a time-uniform confidence sequence is a suitable follow-up, with its own stated assumptions and calibration ([Howard et al.](https://doi.org/10.1214/20-AOS1991)). The profile gate estimates guard stability, not native speedup or compilation payback. Cranelift lowers straight-line numeric functions containing Number parameters and Number constants, moves, `+`, `-`, `*`, `/`, and a terminal return; non-Number constants remain interpreter-only. It also lowers forward-only decision trees that compare Number parameters with Number constants and return static string constants, including Unicode `StringUnits` values. That result is a bytecode Return PC; the VM retrieves the rooted constant, writes the selected Return register, and resumes ordinary dispatch and unwind. All formal parameters are guarded as Numbers before native entry. A failed guard leaves the initialized frame at PC 0, and unsupported bytecode stays interpreted. This uses Futamura-style specialization of fixed residual functions, but does not yet generate the interpreter/compiler from one semantic source as Deegen does.

The current tier is a prototype, not a measured optimization strategy: its exact sequential gate estimates guard stability, but compilation cost, observed time saved and expected remaining calls are not yet included in the payback estimator above. Ershov/Sethi-Ullman scheduling is not yet applied because these functions are short and Cranelift handles native register allocation. The category-theory state relation is expressed through a shared comparison classifier, guard invariant, forward-DAG restriction, rooted return map, and ordinary dispatcher return; it is not a runtime type or compiler abstraction. The string-return tier rejects loops, joins, heap access, calls, exceptions and dynamic return values; all non-Number inputs and unsupported shapes remain in the interpreter. Native candidates are capped at 256 bytecodes and 256 registers; the compiled body has no internal poll, so the shared dispatcher checks cancellation before entering and immediately after returning. Profiling-feature builds leave this JS tier disabled so native execution cannot omit interpreter instrumentation. Unit tests count actual native calls in test builds; normal benchmark reports record mode selection; opt-in `--jit-diagnostics` adds candidate entries, profiles, promotions, compile time, mapped bytes, native/fallback entries, and a static census of conditional branches, `typeof`, strict-comparison and relational-comparison bytecode sites. The census is diagnostic only: these sites are not assumed pure or redundant, and profile data is never used as a don't-care proof. A later Boolean simplifier should operate only on effect-free lowered predicates with path-condition proofs, and must have strict node, cover, compile-time and code-size budgets before any matched JetStream comparison.

Tier metadata is sparse and capped at 256 functions per VM. At most eight functions and 64 KiB of page-rounded executable mappings are retained; unsupported functions, mapping failures and budget exhaustion continue in the interpreter. This bounds executable mappings and profile state, but does not include Cranelift's temporary compilation allocations in the RSS budget. Page-rounded mapping capacity is not a resident-set measurement; benchmark RSS remains the acceptance metric.

## Current measurement limits

The default PATH has Node 24.19.0. The Linux screen used Node 26.10.0 and Bun 1.4.2 from `.local/bench-runtimes`; the M4 comparison is unavailable in this host. The Node 26 build of Quench is 26.12.0. Full JIT qualification still requires the whole suite on both Linux and M4.

The clean-source Linux Web-SSR screen for source revision `4237e8e39` used 11 valid rounds per Quench mode and JIT-enabled Node/Bun (Node 26.10.0, Bun 1.4.2). Quench JIT's median Score was 14.70 versus Bun 129.34 and Node 119.96. Paired median log-score ratios were 0.114 (95% bootstrap interval 0.110–0.119) and 0.122 (0.116–0.133). Quench JIT's median peak RSS was 124.82 MiB versus Bun 232.77 MiB and Node 214.88 MiB; paired RSS ratios were 0.536 (0.522–0.550) and 0.580 (0.576–0.626). Against the same source in interpreted mode, the independent median Score ratio was 0.970 (95% interval 0.929–1.034), which includes a tie; the peak-RSS ratio was 1.002 (0.998–1.007), also including a tie. Quench remains about 8.8× below Bun and 8.2× below Node on score, while peak RSS is lower by 46% and 42%, respectively. This is not a score win or qualification: one of 30 fixtures ran and M4 is unavailable. Benchmark provenance records Quench's `--jit` selection rather than native-entry counts; the targeted runtime test confirms actual native entries for the supported string-return shape. The exact raw reports and 100,000-replicate paired/independent bootstrap summary are under `evidence/jit-jetstream3-2026-10-17/web-ssr-sequential-profile-gate/`. The report's `host.rustc` field is empty because rustc was not on the benchmark shell PATH; the build used rustc 1.99.1. The no-JIT JetStream lane control tracked by Linear TOD-121 remains separate.

The no-JIT audit found no V8-v7 or JavaScript JetStream fixture where Quench beat every measured peer on both Score and peak RSS. Five JetStream Wasm fixtures did beat Bun with JIT disabled on both metrics in 11-round Linux measurements; Node `--jitless` does not expose WebAssembly. I screened those five with Cranelift JIT enabled against JIT-enabled Bun 1.4.2 and Node 26.10.0 on the same Linux host. Every fixture validated and produced equal output, but the following root-closure experiment was one round per fixture and is diagnostic only:

| Wasm fixture | Quench no-JIT vs Bun no-JIT (Score / RSS MiB, 11 rounds) | Quench JIT (Score / RSS MiB, 1 round) | Bun JIT | Node JIT | Screen result |
| --- | ---: | ---: | ---: | ---: | --- |
| HashSet | 21.96 / 35.2 vs 5.90 / 42.1 | 12.27 / 91.0 | 210.92 / 53.8 | 228.02 / 127.0 | loses score to both; RSS only beats Node |
| gcc-loops | 17.69 / 37.2 vs 4.00 / 42.9 | 47.24 / 46.8 | 164.42 / 53.4 | 174.19 / 91.8 | beats both RSS bars, loses both scores |
| quicksort | 40.51 / 27.0 vs 12.98 / 40.2 | 146.76 / 41.8 | 235.95 / 45.6 | 376.42 / 88.4 | beats both RSS bars, loses both scores |
| tsf | 12.07 / 52.4 vs 5.47 / 52.6 | 6.81 / 224.8 | 184.09 / 137.5 | 193.58 / 145.2 | loses both metrics to both peers |
| zlib | 18.08 / 34.5 vs 6.24 / 46.5 | 9.31 / 112.6 | 80.08 / 98.0 | 81.74 / 111.4 | loses score to both and RSS to both |

The experiment compiled only the direct-call closure of each requested root, rebuilding the shared code mapping when later roots appeared. It did not lower peak RSS and its scores did not beat either JIT peer, so the runtime change was removed rather than retained as a regression. The native-entry test remains and asserts that an eligible first Wasm call actually enters compiled code. Raw reports and the exact one-round command are in `evidence/jit-jetstream3-wasm-2026-10-17/root-closure-screen/`.

The follow-up adds opt-in `--jit-diagnostics` reports for JetStream Wasm. The counters record compiled and interpreted function entries, compile time, emitted and mapped code bytes, and compiled-to-VM callouts; diagnostic runs are excluded from qualification. Compiled Wasm `CallKnown` instructions now stay on the fast lane when their target is unsupported, so an interpreted caller can still enter an eligible native callee. The regression test covers an interpreted root, an interpreted fallback function, and a native leaf. The five latest one-round, uninstrumented screens remain below both JIT peers on Score. They beat one or both peers on RSS in four cases; TSF loses on RSS to both. The emitted code maps are only 8–164 KiB while process peak RSS ranges from 41.5 to 224.5 MiB, so executable mapping size alone does not explain the memory gap. Native-entry and fallback counts, raw reports, and reproduction commands are in `evidence/jit-jetstream3-wasm-2026-10-17/fastlane-screens/README.md`. The data point to hot functions outside the current compiled subset and to compile-time/transient memory as the next profiling targets; this iteration does not meet the score-win goal.

## Clean five-case JIT comparison

After adding fused indirect-call handling, same-pool signature identity checks, and allocation-free common call signatures, I ran the five Wasm fixtures that had beaten Bun with JIT off on Score and peak RSS for 11 matched rounds. The source tree and pinned JetStream checkout were clean; each fixture passed stock validation, produced equal outputs, and carried an input hash. The runner's source-to-fixture mapping handles Wasm module directories, and subset checkpoints bind to the exact fixture list. The production report and paired bootstrap analysis are preserved in `evidence/jit-jetstream3-wasm-2026-10-18/qualification/`.

The ratios below compare Quench JIT with each JIT-enabled peer. They are paired median log-ratios with 95% bootstrap intervals; a score ratio above 1 favors Quench, and an RSS ratio below 1 favors Quench.

| Fixture | Quench median Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 29.62 / 113.1 | 0.147 [0.141, 0.154] | 2.035 [2.019, 2.041] | 0.128 [0.118, 0.141] | 0.892 [0.891, 0.893] |
| gcc-loops | 70.96 / 48.1 | 0.414 [0.384, 0.429] | 0.856 [0.847, 0.866] | 0.400 [0.361, 0.410] | 0.524 [0.524, 0.525] |
| quicksort | 209.84 / 42.0 | 0.605 [0.554, 0.715] | 0.863 [0.860, 0.867] | 0.497 [0.476, 0.538] | 0.476 [0.475, 0.476] |
| tsf | 13.38 / 224.8 | 0.068 [0.062, 0.072] | 1.677 [1.611, 1.692] | 0.071 [0.067, 0.073] | 1.614 [1.547, 1.631] |
| zlib | 13.62 / 112.9 | 0.156 [0.151, 0.162] | 1.126 [1.116, 1.134] | 0.154 [0.151, 0.158] | 1.018 [0.998, 1.021] |

No case wins both metrics against either peer. gcc-loops and quicksort are the only cases that beat both RSS bars; both still lose Score decisively. HashSet's two hot `HashTable::add` functions do enter native code (9.49M and 39.53M entries in the separate diagnostics), yet its Score is 0.147 of Bun's. Quicksort runs native code with no VM callouts in its hot sorting function but reaches only 0.605 of Bun's Score, which points to native code quality or runtime overhead rather than interpreter dispatch alone. The earlier TSF combined counter recorded 2,816,456 compiled-to-VM callouts and 224.8 MiB RSS; reducing those boundaries and attributing non-code RSS are higher-value next steps than broadening the eager supported-opcode set. The attempted broad `unreachable` lowering illustrates the admission risk: it expanded TSF from 156 to 244 compiled functions and code from 178 KiB to 9.2 MiB, while the one-round score fell and RSS rose. That change was reverted. These data keep the requested JIT score-and-RSS goal open; they do not support a win claim.

## Same-program indirect-callee fast path

Revision `744f2f9c9` extends the fused Wasm reference-call helper to invoke an eligible same-program target through `run_compiled` directly. Table bounds, nullability, and canonical signature checks still run first; the target's own instance environment is used, and a missing compiled entry falls back to the ordinary VM call path. The regression test verifies a native caller invokes a nine-parameter native callee and returns the same result. The focused Wasm tests and full runtime suite pass (701 unit tests, 18 integration tests).

The five-case clean 11-round report for this revision is in `evidence/jit-jetstream3-wasm-2026-10-18/indirect-call-followup/`. It is complete and qualification-ready, but no score interval reaches parity with Bun or Node, and no case wins both Score and RSS against either peer:

| Fixture | Quench median Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 30.69 / 113.1 | 0.157 [0.153, 0.174] | 2.028 [2.014, 2.034] | 0.144 [0.137, 0.157] | 0.891 [0.891, 0.892] |
| gcc-loops | 71.94 / 47.9 | 0.417 [0.398, 0.455] | 0.859 [0.847, 0.870] | 0.409 [0.379, 0.443] | 0.522 [0.521, 0.523] |
| quicksort | 184.60 / 41.7 | 0.590 [0.539, 0.607] | 0.853 [0.851, 0.859] | 0.455 [0.391, 0.466] | 0.473 [0.472, 0.473] |
| tsf | 14.28 / 224.6 | 0.074 [0.073, 0.079] | 1.670 [1.650, 1.703] | 0.074 [0.072, 0.079] | 1.542 [1.532, 1.546] |
| zlib | 13.47 / 112.6 | 0.157 [0.148, 0.160] | 1.124 [1.121, 1.133] | 0.155 [0.149, 0.160] | 1.011 [0.992, 1.018] |

gcc-loops and quicksort still beat both RSS bars but remain well below both score bars. HashSet beats Node on RSS only; zlib's Node RSS interval includes parity. This fast path reduces interpreter activation for eligible indirect targets, but the reference-call boundary still enters the VM and does not explain TSF's full score gap. The full candidate result did not change the performance conclusion: native-callout attribution and hot generated-code quality remain the next targets.

## TSF callout attribution

The diagnostic counter was split into imported-function callouts and
reference/table callouts, retaining the old aggregate as their saturating sum.
A clean-source one-round TSF diagnostic on revision `adf1b4077464f2b6932d2142a24c36da7cb6336a`
reports 27,006 import callouts and 2,869,000 reference callouts, for a total
of 2,896,006. About 99.1% of these boundaries are table/reference calls. The
run also compiled 156/244 functions, emitted 177,800 bytes, mapped 180,224
bytes, and reported 137.0 ms compilation time. This is a call-count screen,
not timing evidence: diagnostics add overhead and the report has one round.
The signature parameter copy already uses an inline `SmallVec` for up to eight
types, so a repeated heap clone is not the current bottleneck. The split
counter distinguishes reference/table calls from statically imported calls,
but does not show which reference calls enter compiled callees versus VM
fallback. Raw diagnostic data and reproduction details are in
`evidence/jit-jetstream3-wasm-2026-10-18/callout-attribution/`.

## Sampled native-entry timing

The opt-in `QUENCH_WASM_JIT_STATS` record now samples every 256th native entry
per artifact/function. It reports `[function, sample_count, elapsed_ns]`
tuples in `native_entry_samples`. The timer starts after compilation and
context setup and brackets the native entry/trampoline, so it includes
callouts and trap work but excludes compilation. These are inclusive wall-time
samples: recursive and nested entries overlap, deterministic periodic call
patterns can alias with the sample interval, and linked `CallKnown` machine
calls bypass the entry hook and are charged to the enclosing entry. Use the
data to choose a function for deeper profiling, not as per-function CPU time
or benchmark evidence.

A clean-source, one-round selective diagnostic sampled QuickSort's defined
body index 1 (`Quicksort`, symbol index 3) 12 times for 1.203 ms total, out of
3,112 recorded entries. In GCC loops, the top sampled bodies were
`example25` at 0.673 ms and `example10a` at 0.552 ms. This identifies
QuickSort's recursive sort body as the next codegen inspection target, but
does not isolate its loop-body cost or establish a JIT speed change. Both
fixture outputs validated; the diagnostic report is not performance evidence.
The report, sample mapping and reproduction command are in
`evidence/jit-jetstream3-wasm-2026-10-19/native-entry-sampling/`.

## Deferred indirect-call fallback lookups

Commit `ba75d8978` moved residual-program and void-receiver lookup into the
`call_value` fallback branch of `reference_callout`, so a compiled same-program
target avoids those two unused lookups and an `Rc` clone per call. A new test
covers a compiled zero-argument indirect callee with no result. The full runtime
suite passes (702 unit and 18 integration tests).

The clean-source 11-round follow-up is preserved in
`evidence/jit-jetstream3-wasm-2026-10-18/indirect-call-fallback-cleanup/`.
Every fixture validated and produced equal output, but no Score ratio reached
parity with Bun or Node:

| Fixture | Quench Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 32.51 / 113.2 | 0.153 [0.148, 0.172] | 2.033 [2.026, 2.045] | 0.133 [0.125, 0.134] | 0.892 [0.892, 0.893] |
| gcc-loops | 71.53 / 48.1 | 0.398 [0.384, 0.415] | 0.853 [0.847, 0.867] | 0.391 [0.367, 0.425] | 0.524 [0.522, 0.526] |
| quicksort | 189.91 / 41.9 | 0.549 [0.526, 0.601] | 0.860 [0.858, 0.862] | 0.481 [0.432, 0.509] | 0.475 [0.474, 0.475] |
| tsf | 14.41 / 224.7 | 0.070 [0.066, 0.080] | 1.689 [1.624, 1.695] | 0.072 [0.072, 0.074] | 1.552 [1.536, 1.626] |
| zlib | 13.38 / 112.8 | 0.155 [0.150, 0.160] | 1.125 [1.117, 1.129] | 0.157 [0.146, 0.161] | 0.996 [0.992, 1.015] |

gcc-loops and quicksort beat both RSS bars but remain far below both score
bars; HashSet beats Node on RSS only; TSF loses on both metrics; zlib's Node
RSS interval contains parity. This keeps the requested JIT score-and-RSS goal
open. The direct same-instance native-entry path now bypasses argument boxing
and nested `run_compiled` setup while retaining table bounds, null, signature,
memory-guard, and cross-instance checks. Targeted correctness tests prove the
shortcut runs for same-instance calls and stays on fallback for a foreign
instance; clean benchmark qualification is pending.

The remaining design work follows a staged-specialization path: keep validated
Wasm and residual JS bytecode as the static input, specialize only hot
operations and call sites whose guards pay for compilation, and make fallback
an explicit continuation into the existing VM. This is the practical first
Futamura projection rather than a general-purpose self-applicable partial
evaluator. V8's official Wasm pipeline likewise separates fast baseline
compilation from selective top-tier compilation, while the Deegen paper's
single semantics source and Copy-and-Patch baseline suggest a later route to
lower compile cost if measurements show compiler allocation is still the RSS
driver. These sources motivate tiering and specialization; they do not predict
Quench's benchmark outcome. [V8 Wasm compilation pipeline](https://v8.dev/docs/wasm-compilation-pipeline), [Deegen paper](https://arxiv.org/abs/2411.11469), [Futamura's partial-evaluation paper](https://www.jstage.jst.go.jp/article/jssst/21/5/21_5_343/_article/-char/en).

## Per-site TSF indirect-call attribution

An opt-in diagnostic ABI records each compiled indirect call site's total,
same-program native-target hits, and VM fallback/trap resolutions. The ordinary
JIT ABI and generated call path remain unchanged, and the diagnostic report is
excluded from qualification. A one-round TSF diagnostic reports 2,869,000
reference calls: 2,859,000 (99.65%) reached same-program compiled functions,
while 10,000 used VM fallback/trap handling. The hottest site is runtime
function 221 at PC 63 (`read` in the pinned TSF symbols), with 2,663,108 native
target hits and no fallback. The complete report and source/binary hashes are in
`evidence/jit-jetstream3-wasm-2026-10-18/site-attribution/README.md`.

This confirms a high-volume VM transition even when both caller and target are
compiled. Commit `3399bb547` calls the existing fixed native entry directly
for same-program, same-instance targets using raw Wasm words. Table bounds,
null, signature, memory-guard, and cross-instance checks stay in the VM; other
targets retain the existing fallback. The tests prove direct entry, guard-trap
propagation, nested raised-error propagation, and foreign-instance fallback.

The clean five-case follow-up used 11 matched rounds on Node v26.12.0 and Bun
1.4.3. It is preserved in
`evidence/jit-jetstream3-wasm-2026-10-18/direct-entry/`. All fixtures validated
and produced equal output, but no fixture passed the Score-and-RSS rule:

| Fixture | Quench Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 32.21 / 112.87 | 0.157 [0.147, 0.167] | 2.289 [2.278, 2.300] | 0.136 [0.134, 0.141] | 1.065 [1.064, 1.065] |
| gcc-loops | 72.94 / 47.68 | 0.422 [0.404, 0.446] | 0.981 [0.957, 0.984] | 0.398 [0.384, 0.420] | 0.669 [0.669, 0.672] |
| quicksort | 206.68 / 41.46 | 0.604 [0.501, 0.686] | 0.985 [0.978, 0.987] | 0.473 [0.455, 0.533] | 0.608 [0.605, 0.608] |
| TSF | 14.73 / 224.47 | 0.076 [0.069, 0.079] | 1.881 [1.838, 1.890] | 0.077 [0.074, 0.080] | 1.883 [1.783, 1.903] |
| zlib | 13.65 / 112.55 | 0.154 [0.150, 0.155] | 1.031 [1.029, 1.032] | 0.159 [0.152, 0.165] | 1.224 [1.194, 1.232] |

gcc-loops and quicksort retain their RSS wins but remain far behind on Score.
The direct-entry optimization does not materially close the TSF score or RSS
gap. The next target is HashSet's interpreted `main`: its compiled callees
receive 49 million `CallKnown` entries across the benchmark. The bounded root
compilation screen below tests that caller path; `WasmUnreachable` remains
opt-in because the prior global experiment expanded TSF code about 52× and
increased both compile time and RSS.

## Monomorphic Wasm indirect-call cache

Commit `f40e5ddeda1765ace5506af81a37fddb1b6de2b0` adds a lazy monomorphic cache
to compiled `call_indirect` sites. It checks the table binding, index,
signature, caller environment, and current table element; target artifacts are
weakly held. Every hit rebuilds the memory and global activation state, while
every miss retains the ordinary resolver's bounds, null, signature, and
fallback behavior. GC clears cached identities before freed heap slots can be
reused. The mutable-table test proves that a warmed site observes replacement,
that the cache is actually hit, and that a null target still traps.

The clean-source 11-round QuickSort/gcc-loops screen is preserved in
`evidence/jit-jetstream3-wasm-2026-10-18/indirect-call-cache/`. Output matched
on both fixtures. Quench remains below Bun and Node on Score (QuickSort 190.49
vs. 345.96/405.88; gcc-loops 64.47 vs. 186.70/192.71), while its max RSS is
lower on both. Paired Score ratios are 0.569 [0.520, 0.606] and 0.465 [0.416,
0.500] against Bun/Node for QuickSort, and 0.340 [0.324, 0.373] and 0.342
[0.329, 0.350] for gcc-loops. This targeted screen is not a score win; the
next optimization should address the hot-call tiering path and first-iteration
compile cost.

## Bounded HashSet root compilation

Commit `ea7c76c6c327f76f18376b75ab278c22ce7c585e` adds an explicitly selected,
bounded Wasm root mode. It compiles the transitive `CallKnown` closure for the
requested residual function and permits `WasmUnreachable` only in that mode,
lowering it to the ordinary Wasm trap. Known callees outside the native plan
use the existing VM callout and may re-enter another compiled function. The
callout converts defined-local IDs to absolute Wasm function indices; a
regression covers an imported function before the defined root. Multi-result
fallbacks are rejected because the current callout ABI returns at most one
value. The direct-entry raw argument path also marshals directly from the
caller register window into owned Wasm words before native entry.

The root mode has limits of 128 selected functions, 80,000 bytecodes per
function, and 2 MiB emitted code. It is opt-in with
`--jit-wasm-root <residual-function-id>` and never marks a report eligible for
qualification. The one-round HashSet diagnostic confirms root 3 entered native
code (50 times); functions 22 and 23 also entered. It compiled 22 of 35
functions, emitted 1.30 MiB (1,357,938 bytes) of code, and took 1.28 seconds
to compile.

The clean, uninstrumented one-round screen is in
`evidence/jit-jetstream3-wasm-2026-10-18/root3-bounded-experiment/`. It passed
validation and output equality, but measured Quench at 54.45 Score / 305.3 MiB
RSS, Bun 1.4.3 at 207.01 / 50.6 MiB, and Node 26.12.0 at 202.50 / 110.9 MiB.
The score rises relative to the earlier HashSet JIT result, but this is an
unpaired one-round screen; the RSS cost grows by about 190 MiB, consistent with
a large compilation-footprint cost. It loses both metrics against both peers,
so it was not expanded to 11 rounds. This Futamura-style specialization is
useful for pinpointing the hot caller, but this Cranelift admission strategy is
not suitable for the requested score-and-RSS win. Lower-memory baseline code
generation or reducing Quench's JIT-mode base footprint is the next research
direction.

The root selector also supports two distinct experimental admission policies.
`--jit-wasm-root-only` with `--jit-wasm-root <residual-function-id>` compiles
only that function and defaults Cranelift `opt_level=none`; `--jit-wasm-root-first`
starts with the selected function at `speed`, then lets hot fallback callees use
the existing bounded promotion tier. Either selected root can override its
compile level with `QUENCH_WASM_JIT_ROOT_OPT_LEVEL=none|speed|speed_and_size`.
The strict mode is useful for measuring the cost of one native entry; root-first
tests whether the selected root can benefit from a smaller first compile while
retaining tier-up. The CLI records separate checkpoint/report fields and
provenance for both, and neither mode is eligible for qualification. Root-first
promotion is enabled only if the selected root compiles successfully; a rejected
root leaves the workload interpreted. Their runtime switches are
`QUENCH_WASM_JIT_ROOT_ONLY=1` and `QUENCH_WASM_JIT_ROOT_FIRST=1`, respectively.

On JetStream 3 QuickSort, root-only function 1 at `speed` scored 292.02 versus
247.90 at its default `none` level over separate 11-round clean-source Linux
windows; median RSS was 41.50 versus 41.36 MiB. Against full eager JIT in a
matching 11-round window, root-only `speed` scored 292.02 versus 237.33 and used
41.50 versus 42.83 MiB. It remained below Bun's 393.36 score, while using less
RSS than both Bun (47.72 MiB) and Node (88.38 MiB). This is a promising opt-in
tiering/codegen tradeoff, not a peer score win or qualification result; the
root-only mode is intentionally excluded from qualification. See
`evidence/jit-jetstream3-wasm-2026-10-19/root-only-opt-level/` for the reports
and reproduction details.

The root-first GCC-Wasm experiment selected residual function 19, then allowed
hot fallback callees to use the bounded tier. An 11-round Linux screen scored
58.62 / 48.20 MiB versus Bun 174.49 / 54.10 MiB and Node 179.90 / 62.38 MiB.
Paired Score ratios were 0.351 [0.327, 0.359] vs Bun and 0.329 [0.324,
0.345] vs Node; RSS ratios were 0.893 [0.881, 0.910] and 0.773 [0.772,
0.773]. The mode preserves Quench's RSS edge but remains far below both JIT
Score bars and the prior selective-tier GCC screen. Keep it as an opt-in
diagnostic policy, not a default tier strategy. Evidence and reproduction
commands are in `evidence/jit-jetstream3-wasm-2026-10-18/root-first/`.

The next GCC hypothesis is narrower and higher leverage: diagnostics
attributed 307,270 of 309,039 imported callouts to `_emscripten_memcpy_js`.
W-08 tested a same-memory direct copy inside the existing VM boundary, guarded
by the exact helper, current captured view and method, active unshared Wasm
memory identity, and copyWithin-compatible signed index clamping. Every failed
guard keeps the ordinary JavaScript path. The callout's interrupt, stack,
roots, realm, panic, and cleanup behavior stays in place. Its focused runtime
test passed, including overlap, extreme signed indices, stale and reloaded
views after memory growth, alternate buffers, replaced methods, and cleanup.

The cleanly paired 11-round GCC-Wasm diagnostic A/B found median Score 53.23
with the late direct-copy path enabled vs. 51.71 disabled. The paired ratio was
1.0425 (95% interval 1.0263–1.0750), below its predeclared 1.10 keep
threshold; peak RSS was effectively unchanged at a paired ratio of 1.0000
(0.99935–1.00049). Diagnostics recorded 307,300 direct copies out of 308,232
import callouts. Quench retains lower RSS than Bun and Node but remains far
below their Scores: the enabled arm reached 0.314 of Bun's and 0.295 of Node's
paired Score. The late-bound optimization is rejected as insufficient in
isolation. Its diagnostic report, raw A/B, analysis, and reproduction command
are in `evidence/jit-jetstream3-wasm-2026-10-18/direct-emscripten-copy/`.

TOD-143 tests the next boundary: recognize the exact imported helper and
perform the same dynamic guards on raw Wasm i32 arguments before building
`SmallVec<Value>`. On guard miss it keeps the existing import path. This targets
the measured argument conversion and dispatch work; it is not an expected RSS
optimization. The paired A/B keeps the late direct path enabled in both arms
and toggles only raw-i32 admission. It remains a diagnostic experiment, and a
peer win still requires Score and RSS intervals to favor Quench.

The raw-i32 path was exercised: its one-round diagnostic recorded 307,300 hits
out of 308,232 import callouts, with no late-path hits. The 11-round A/B scored
53.82 vs. 53.61 with raw admission off; its paired Score ratio was 1.0211
(95% interval 0.9814–1.0491), below the predeclared 1.25 keep threshold and
including a tie. RSS stayed flat at 0.99984 (0.99943–1.00041). The implementation
remains opt-in and disabled by default for reproducibility, but does not count
as a performance win. Quench remains below Bun and Node on Score at 0.318 and
0.291, respectively, while keeping lower RSS. Full raw samples and analysis
are in `evidence/jit-jetstream3-wasm-2026-10-18/raw-emscripten-copy/`.

TOD-144 adds an opt-in lowering for QuickSort's comparison-only Wasm branches.
The compiler passes signed or unsigned Cranelift `icmp` conditions directly to
the existing branch emitter; value-producing comparisons are unchanged, and
the taken loop-backedge interrupt safepoint remains in place. The focused
regression covers register and immediate comparisons at i32 extrema, both
outcomes, and the full runtime suite still passes. Cranelift's vendored x64
lowering already recognizes both the old `uextend(icmp)` branch input and the
direct `icmp` shape, so the source change was not expected to guarantee a
different machine instruction sequence.

The three-round QuickSort screen improved the median Score by 20.3%, but its
paired interval included a tie, so the predeclared 11-round confirmation ran.
That trial scored 211.41 with direct compare branches on vs. 217.46 off: a
paired ratio of 0.9693 (95% interval 0.9362–1.0238). Peak RSS was effectively
flat at ratio 1.0031 (0.9947–1.0092). The candidate remains below Bun on Score
(0.5914, 0.4953–0.6592) and Node (0.5045, 0.4492–0.5094), while retaining lower
RSS than both. Leave the feature opt-in and disabled by default; it does not
meet the 1.02 keep threshold or establish a peer win. Reports and reproduction
commands are in
`evidence/jit-jetstream3-wasm-2026-10-18/direct-compare-branch/`.

The next optimization should focus on emitted QuickSort work rather than
branch-condition representation, or return to gcc-loops' measured import-call
hotspot with a lower-cost boundary specialization.

TOD-145 tests a narrower staged specialization at that import boundary. When
enabled, the compiled callout recognizes only the matching Emscripten memcpy
function index, signature, and per-instance host binding, then passes its raw
Wasm i32 arguments to the existing dynamic current-view and bounds guards. A
miss falls through to the ordinary Wasm-to-JS import path. Stack and interrupt
gates still run before the copy, and the enclosing callout retains panic
handling, active-context restoration, memory refresh, and completion behavior.
This is a Futamura-style partial evaluation of known import metadata around
dynamic arguments: the fixed import identity is staged, while memory state,
argument ranges, and observable copy semantics remain checked at runtime. It
reduces generic dispatch work without adding a second VM or changing module
semantics. The early-import switch is opt-in and the experiment is diagnostic;
it qualifies only if paired Score improves with its confidence interval above
a tie and peak RSS stays within the predeclared 0.5 MiB budget.

The early path was exercised on the target: a separate counter recorded
307,300 successful early admissions among 308,232 compiled import callouts.
Despite that coverage, the three-round paired screen regressed: Score was
50.51 with early admission vs. 56.48 without, a ratio of 0.9527 (95% interval
0.8943–0.9661). Peak RSS was effectively unchanged at ratio 1.0004
(0.9996–1.0027). This fails the predeclared keep threshold, so no 11-round
confirmation was run. Leave the switch disabled and do not count the feature
as a win. The result shows that resolving and checking the early binding costs
more than the generic import setup it avoids on gcc-loops. Raw samples, paired
analysis, and path-coverage diagnostics are in
`evidence/jit-jetstream3-wasm-2026-10-18/early-emscripten-copy/`.

## Sparse analysis and compile-cost screens

Commit `v2-jit` replaces the per-PC dense register-state vectors in Wasm JIT
analysis with sorted sparse deviations from the default i32 slot. Joins retain
the same `Slot` lattice: a deviation meeting the default or a different value
becomes `Conflict`. A focused unit test covers equal facts, differing facts,
default-path disagreement, out-of-range registers, and resetting a slot to its
default. Linux-only, opt-in JIT diagnostics now report the analyzer's modeled
state storage and elapsed time, plus per-function Cranelift compile time and
process RSS/high-water snapshots.

For HashSet `main` (function 3), the diagnostic measured 66,145 reachable
bytecodes and 97 registers. The dense-equivalent state payload is 76,992,780
bytes; sparse states and allocated capacities use 6,962,568 bytes, about 11.1x
less. Planning takes 5.7-10.2 ms. The Cranelift body remains the large cost:
with optimization disabled it takes 875 ms and the process reaches
266,068 KiB high-water RSS. Cranelift's `none` setting reduces compile time and
RSS, but its one-round root-3 screen is 57.00 Score / 259.1 MiB. The
`speed_and_size` screen is 58.32 / 284.7 MiB, and default `speed` is 58.70 /
285.0 MiB. These are root-selected one-round screens, not matched comparisons
or qualification results.

JetStream includes the first, worst, and average per-fixture scores in its
geometric-mean Score. The diagnostic root-3 run spends 944 ms compiling and
emits 1,544,754 bytes. Its first timed iteration takes 1,005 ms (Score 4.98),
while the later average is 22.60 ms (Score 221.24). HashSet loads its Wasm
binary in `init()` but first initializes the Emscripten module inside the first
timed `runIteration()`. Quench then compiles the whole eligible program on the
first exported-function entry. This identifies cold compile latency, as well
as Cranelift's transient memory, as the immediate blocker; eager compilation
at instantiation would only move this cost because instantiation itself is in
the timed interval. The direct next compiler milestone is a fast single-pass
baseline tier with interpreter fallback, followed by selective Cranelift tier
up when expected execution savings justify code size and promotion cost. This
matches the division of work in V8 Liftoff/TurboFan and Wasmtime Winch, and the
low runtime compiler cost targeted by Deegen/Copy-and-Patch; those systems do
not predict Quench's performance. [Cranelift optimization levels](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/settings/struct.Flags.html),
[Wasmtime fast compilation](https://docs.wasmtime.dev/examples-fast-compilation.html),
[V8 Liftoff](https://v8.dev/blog/liftoff),
[Deegen](https://compilers.stanford.edu/software/deegen/).

Narrower root-31 (`operator new`) and root-27 screens use about 44 MiB RSS,
below both JIT peers in these one-round runs, but score only 24.15 and 23.48.
They therefore confirm a memory opportunity without a score win. Full commands,
environment wrapper, diagnostic details, and raw reports are in
`evidence/jit-jetstream3-wasm-2026-10-18/sparse-analysis-baseline-screen/`.
No case has yet met the paired score-and-RSS rule; the target remains open.

## Selective handoff from `CallKnown`

Commit `7961b7bde` lets a hot eligible Wasm `CallKnown` activation leave the
frameless fast interpreter lane and reach the existing one-shot Cranelift
promotion gate. Cold calls stay on the lane, terminal calls retain the
existing tail path, and the general dispatcher remains the sole owner of the
compile attempt. The regression uses a four-function chain so the frontend
does not inline away the target; it checks the residual `CallKnown`, correct
result, tier threshold, and native entries.

The selective-mode one-round diagnostic and an eleven-round uninstrumented
paired A/B now isolate this handoff on gcc-loops. In the eleven-round run,
selective Quench scored 67.45 versus 54.63 for default JIT, a paired ratio of
1.236 [1.195, 1.275]. Peak RSS changed from 47.77 to 47.50 MiB, ratio 0.9942
[0.9939, 0.9966]. The three-round screen showed the same direction: score
ratio 1.272 [1.242, 1.280] and RSS ratio 0.9958 [0.9916, 0.9967]. Selective
Quench still scored only 0.358 of Bun and 0.347 of Node; its 47.50 MiB RSS
was lower than Bun's 55.18 MiB and Node's 92.08 MiB. This passes the local
advancement gate, but does not meet the required score-and-RSS win against
other JIT runtimes. Experimental A/B reports remain explicitly nonqualifying.
Raw reports, paired analyses, and reproduction commands are in
`evidence/jit-jetstream3-wasm-2026-10-19/selective-gcc-handoff/`.

The instrumented one-round mechanism check recorded 75 selective-tier
compilation events, 352,517 summed emitted code bytes, and 286.5 ms summed
compile time, versus 8 events, 569,416 bytes, and 603.1 ms in default mode.
Both still recorded about 309,000 import callouts (309,039 selective,
309,150 default). These counters show that the handoff activates while the
import boundary remains a substantial cost; diagnostic scores and timings
are not performance evidence. The aggregate does not attribute individual
import symbols. Selective tier-up is only an admission mechanism; it does not
by itself close the peer-score gap.

The follow-up adds per-import attribution to `QUENCH_WASM_JIT_STATS`, gated by
the existing diagnostics switch. A clean-source, one-round selective GCC
diagnostic attributes 307,270 of 309,039 imported callbacks to
`env._emscripten_memcpy_js` and 1,769 to `env.emscripten_date_now`; the symbol
counts sum to the aggregate on every compiled artifact. The counter records
the Wasm function index and original import declaration index, since other
import kinds may interleave in the module section. This is boundary-coverage
evidence only. The early-import screen lost Score, and the leaf screen
recorded zero native-leaf hits, so this attribution does not support another
memcpy-specific specialization. The raw report, binary hashes, and command
are in `evidence/jit-jetstream3-wasm-2026-10-19/import-callout-attribution/`.

## Safepoints on compiled control-flow cycles

Commit `e955cff99` retains the native function-entry poll and checks a
conditional branch only when its taken edge moves to an earlier bytecode PC.
For a finite bytecode graph, every cycle contains such a backward edge; a
forward-only path cannot remain in the loop indefinitely. This moves the
interrupt effect onto cycle-crossing edges, preserving cancellation at loop
iteration granularity while removing atomic loads from forward exits. The
native-loop cancellation regression requests termination from a second thread
after the Wasm loop reaches an imported callback.

The clean 11-round comparison still confirms lower RSS than Bun and Node on
gcc-loops and QuickSort, but its Score intervals remain below parity. Quench's
medians are 59.80 / 47.79 MiB and 173.19 / 42.43 MiB respectively. Because the
earlier report used a separate sampling window, these runs do not establish a
causal before/after gain from safepoint placement. Full rounds, paired
intervals, and reproduction commands are in
`evidence/jit-jetstream3-wasm-2026-10-18/backedge-safepoints/`.

The gcc-loops diagnostic attributes 307,270 of its 309,039 imported callouts
to `_emscripten_memcpy_js`. The generic boundary was cloning parameter and
result vectors and allocating converted JS arguments on each call. Commit
`280b9a011` stores these small lists inline with `SmallVec`, spilling for
larger imports while retaining the per-instance target, normal JavaScript
conversion, dynamic helper lookup, and fallback semantics. It avoids
specializing behavior by import name, since Emscripten's helper reads the
current memory view and property target on every call. The consecutive
same-host 11-round A/B screen is inconclusive for Score: the medians shift down
1.3% on gcc-loops and 3.2% on QuickSort, but Bun's scores also fall between
runs. Peak RSS is lower by 0.83 MiB and 1.06 MiB; Quench still loses Score to
both engines. Full reports and executable hashes are in
`evidence/jit-jetstream3-wasm-2026-10-18/import-alloc-smallvec/`. The
optimization removes container churn, not the Wasm-to-VM-to-JS callout;
reducing that boundary remains a separate opportunity with realm, caller
masking, and dynamic property semantics to preserve.

## Wrapped indexed-memory addresses

Commit `ee75becfe` zero-extends the Wasm `i32` effective address after its
32-bit arithmetic on the repeated-memory path. The boundary regression checks
wrapped loads/stores, alias coherence, carried out-of-bounds addresses,
access-width traps, and nonzero memarg carries. This also exposes a codegen
tradeoff: Quicksort function 1 shrinks from 573 to 476 native bytes and reuses
the formed address for swap stores, while indexed loads still need a separate
address-forming instruction.

In two consecutive but separately randomized 11-round windows, QuickSort
Score moved from 163.89 to 232.81; an independent bootstrap of the round
samples gives a candidate/baseline ratio of 1.421 (95% interval 1.280–1.561).
Normalizing each window to its paired Bun samples gives a 1.344× relative
improvement (1.201–1.536). This is a strong QuickSort gain, but not a
randomized per-round A/B. GCC Score stayed flat (58.47 to 57.83), and peak
RSS did not materially change. The candidate still scores below Bun and Node
on both fixtures, while keeping lower peak RSS. The raw reports, executable
hashes, peer-normalized comparison, and machine-code notes are in
`evidence/jit-jetstream3-wasm-2026-10-18/indexed-address-zeroextend/`.

## Persistent same-artifact indirect dispatch

The artifact-local `call_indirect` cache now keeps a per-site monomorphic
certificate in native code. A resolver miss still performs the VM's full table,
null, and Wasm signature checks; it seeds the certificate only after a
successful same-artifact call, a strong root across re-entrant VM execution,
and a final live-function and table-slot identity check. The native fast path
guards the VM epoch, environment, and dynamic table index before loading the
cached function entry. Table writes and collection advance the epoch, so a
mutation or GC during a re-entrant call falls back to the resolver on the next
execution. This is a small feedback-directed partial evaluation of a stable
site, in the direction of Futamura's first projection, while preserving the
validated bytecode and ordinary resolver as semantic authority. The cache is
monomorphic; JetStream 3's polymorphic-call work points toward bounded
multi-target inlining only after profiling and code-size payback justify it.

The final 11-round five-fixture screen is recorded in
`evidence/jit-jetstream3-wasm-2026-10-18/persistent-indirect-cache/`. Quench
retains an RSS advantage over Bun on HashSet, gcc-loops, QuickSort, and zlib,
but its Score remains below both Bun and Node on all five fixtures. TSF is
still the memory outlier at 225.62 MiB versus Bun's 126.09 MiB and Node's
145.27 MiB. Thus this cache reduces repeated resolver boundaries, but does not
yet meet the JIT score-and-RSS goal. A one-round selective-tier diagnostic
screen reported 8.32 Score / 226.0 MiB; that run is not comparable to the
non-diagnostic 11-round default report and establishes no selective-mode
effect. Selective mode remains experimental and needs a matched screen before
any memory claim.

The regression suite checks the JS import boundary with three inline i32
arguments and an i32 result. The direct-cache publication path keeps the
target rooted across native execution, including re-entrant VM calls. The
mechanism keeps the state relation explicit: cached dispatch must commute with
slow resolution for the same environment, index, epoch, table target,
signature, and compiled entry. This is a local correctness argument, not a
claim that category theory or the cache itself guarantees a benchmark win.
The next optimization needs to target the dominant generated-code or runtime
cost rather than adding speculative tiers. JetStream 3 emphasizes polymorphic
indirect-call inlining and code-size-aware decisions; Deegen and Copy-and-Patch
motivate a future single-source, low-cost baseline compiler if Cranelift
compilation remains a measured cost. [JetStream 3](https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/),
[Deegen](https://compilers.stanford.edu/software/deegen/),
[Copy-and-Patch](https://compilers.stanford.edu/software/copy-and-patch/).

## Indexed i32 offset reuse experiment

The indexed i32 offset-reuse experiment tested the exact relation
`(x ± y) * 4 mod 2^32 = (x * 4 ± y * 4) mod 2^32` in two compiler variants.
Both kept address formation, bounds checks, trap order, and loop safepoints on
the existing path and passed interpreter/compiled-off/compiled-on smoke checks
for ordinary, wrapped, and out-of-bounds indices. The first variant tracked
every register in structurally matching functions; the second used a
backward-use slice from `i32.shl 2` operands.

Neither variant passed the predeclared three-round QuickSort screen. The
first's paired Score ratio was 1.0395 [0.8684, 1.1094], below the 1.05 gate
and crossing a tie. The narrower variant regressed, with a paired ratio of
0.8320 [0.8075, 0.8383]; RSS was effectively unchanged. Neither advanced to
eleven rounds, and neither beat Bun or Node on Score. The compiler switch and
diagnostic runner option were removed. Full runs and the rejection decision
are recorded in
`evidence/jit-jetstream3-wasm-2026-10-18/indexed-offset-reuse/`.

## GCC-Wasm four-lane affine pair experiment

The GCC `example1` loop computes two independent adjacent `i32` sums per
backedge. A restricted matcher proves static non-aliasing, memory32 range
validity, a zero-start induction variable, and safe loop entry. The candidate
uses Cranelift's supported `I32X4` operation, stores results in the original
two-element order, and polls interrupts at both original backedges. An initial
`I32X2` prototype could not be lowered by Cranelift 0.134.3's x64 backend and
was discarded before timing.

The valid three-round paired screen passed JetStream output validation but
regressed Score: candidate/baseline was 0.927 (paired bootstrap 95% interval
0.862–0.973). Peak RSS was effectively flat at 0.995 of baseline
(0.992–0.997). The candidate scored 53.24 versus Bun's 184.91 and Node's
180.36, while using 47.91 MiB versus 56.11 MiB and 92.01 MiB. Since the
optimization regressed against the same Quench binary, it did not advance to
eleven rounds. It remains available only by explicitly setting
`QUENCH_WASM_JIT_AFFINE_PAIR=1`; the default path leaves it off. The
instrumented one-round run only confirms native compilation and equal output;
it is not timing evidence. Reports and paired analysis are in
`evidence/jit-jetstream3-wasm-2026-10-18/affine-pair/`.

## QuickSort root-first admission screen

Three-round default and explicit root-9-first screens both passed output
validation, but neither improved QuickSort Score. The root-9 screen recorded
211.07 versus 219.12 for the default policy with the same 43.09 MiB Quench
peak RSS; both remained below Bun and Node on Score. This admission-policy
variant was rejected without changing production selection. Reports and the
directional result are in
`evidence/jit-jetstream3-wasm-2026-10-18/root-first/`.

## Same-artifact body-pointer cache screen

The TSF diagnostic A/B compared the same production executable with the
same-artifact indirect-call body-pointer cache disabled and enabled, rotating
Quench, Bun, and Node over three valid rounds. All outputs matched. Enabling
the cache changed Quench's median Score from 10.95 to 10.40 (paired ratio
0.960, 95% interval 0.920–1.013) and left peak RSS effectively unchanged
(ratio 1.000, interval 0.9998–1.0002). The interval includes a tie, so this
does not establish a regression or a gain; TSF remains far below both peers
on Score and above them on RSS. The cache is now opt-in via
`QUENCH_WASM_JIT_REFERENCE_BODY_CACHE=1` pending a workload that demonstrates
payback. The screen is diagnostic rather than qualification evidence. Raw
rounds and paired analysis are in
`evidence/jit-jetstream3-wasm-2026-10-18/reference-body-cache/`.

## GCC-Wasm native Emscripten memcpy leaf screen

The next boundary experiment emitted a direct native leaf for the exact
`_emscripten_memcpy_js(i32, i32, i32) -> ()` import. It used the existing
live-view, method, and unshared-memory guard on the slow path, and revoked the
raw-memory certificate at import and indirect-reference callouts. After a
provenance review, both paired arms enabled the same direct, raw-i32, and
early-import paths, with only the leaf switch differing. The corrected
three-round screen had valid equal outputs, but leaf-on Score was 50.02 versus
51.20 off (paired ratio 0.977, 95% interval 0.944–1.024) and peak RSS was flat
(0.99984, 0.99829–1.00016). The interval includes a tie, so there was no
supported speedup; the screen did not advance to eleven rounds. Quench's
leaf-on Score was 50.02 versus Bun's 172.94 and Node's 177.67, while Quench
used less peak RSS than both peers.

The instrumented mechanism check found 307,300 early memcpy imports but zero
native leaf hits. Both arms counted the same 307,300 early imports; the summed
compiled code was 1,280 bytes larger with the leaf switch, but the specialized
path was not reached. More call-pattern instrumentation is needed to
distinguish cache-scope limits from a call-site or guard mismatch. Keep the
switch as an opt-in experiment; it is not a default optimization, and advance
it only if a safe certificate reaches hot calls and a fresh paired screen
supports it. An earlier screen compared different import paths and is retained
as a confounded result, not evidence for the leaf. Corrected reports, the
hit-count diagnostic, and reproduction commands are in
`evidence/jit-jetstream3-wasm-2026-10-18/memcpy-native-leaf/`.

## JavaScript `hash-map` guarded `equals` specialization

The `equals(a, b)` helper is called 54 million times in JetStream 3's
`hash-map` fixture. Under the existing Number-entry guard, its `typeof`
decision tree can be partially evaluated: internal compiler-owned locals and
static type-name comparisons are folded, while the final numeric equality is
emitted as native code. Non-Number calls resume in the interpreter. The
compiled Boolean result returns through the existing frame so shared-VM
unwinding stays intact.

The one-round JIT diagnostic passed stock fixture output validation. It
compiled `equals` once (8.5 ms, 4,096 mapped code bytes) and recorded
53,999,969 native entries. Quench scored 1.54 with 71.8 MiB peak RSS in
270.6 s; Bun scored 364.13 at 93.8 MiB and Node 153.63 at 161.0 MiB. A prior
one-round Quench screen scored 2.46 in 239.1 s. This is not a speed win and
the two screens are not a paired comparison. The native body is tiny, but the
per-call VM frame, entry guard, and return transition remain; compiling the
callee alone does not remove those boundaries.

The current entry tier remains an experiment until its performance value is
established. Raw one-round data was captured at
`target/jit-hash-map-after.json` in the working tree; that diagnostic includes
source-dirty state and must not be treated as qualification evidence.

The follow-up adds a same-program fast path for already-compiled callees. At a
plain `Call`, the runtime checks the existing tier and Number arguments before
activating a frame; cold calls, non-Number arguments, other programs, tail
calls, and interpreter mode retain the normal VM path. On a second one-round
screen, output validation passed and Quench scored 2.15 at 71.7 MiB in 253.3 s.
That is directionally faster than the earlier 1.54 / 270.6 s frame-entry
screen, but the screens were unpaired and Bun/Node timings also shifted. It
does not establish a gain, and Quench remains far behind both runtimes. The
diagnostics still show `runIteration` unsupported; compiling `equals` does not
remove the interpreter loop around the 54 million calls. A controlled paired
comparison is now the next step before keeping this fast path. Its diagnostic is
`target/jit-hash-map-direct-call.json` in the working tree.

## Statically-known JavaScript calls

The interpreter's `CallKnown` opcode previously always created a callee
activation, even when the same program already held a guarded native artifact.
In compiled mode, non-tail JavaScript `CallKnown` now checks that artifact
before creating the frame and enters it directly when its arguments satisfy the
existing guard. Artifacts that need a callee frame, including lexical-capture
and string-loop entries, continue through normal activation so capture values
and specialized ABIs are never read from an incomplete argument buffer. Wasm,
tail calls, unsupported arguments and interpreter mode keep their existing
paths.

The focused JIT tests pass 16/16, including a regression that confirms a hot
`CallKnown` numeric callee reaches native code after promotion. The first clean
three-round Web SSR screen after this change scored 14.65 with 132.1 MiB median
RSS; a prior three-round screen scored 13.46 with 131.3 MiB. These are separate
screens and do not establish a paired gain. A one-round stock `hash-map` screen
validated and recorded Quench at 4.35 Score / 74.3 MiB, Bun at 467.12 / 95.9
MiB and Node at 414.81 / 161.4 MiB. The RSS bar is below both peers, while
Score remains far behind; this is diagnostic progress, not a benchmark win.
Raw report: `evidence/jit-jetstream3-2026-10-20/known-call-fast-path/hash-map-jit-1round.json`.

## Guarded Int32 identity return

The JS tier now recognizes an effect-free Number path through a `typeof`
switch when it checks `(arg | 0) == arg` and returns the original argument.
The matcher follows only the Number dispatch path, rejecting effects on that
path while allowing effects in skipped type arms. At runtime, an Int32 guard
protects the native identity entry; fractions, strings, objects and values
outside the Int32 range stay in the ordinary interpreter. A focused regression
test covers both the native integer arm and those fallback cases.

On a one-round diagnostic run of JetStream `hash-map`, `computeHashCode`
compiled once and took 64.8 million native entries. `_findKeyEntry` still took
54 million interpreter entries and about 26.9 seconds inclusive. In the
separate clean one-round screen Quench scored 3.51 at 73.9 MiB, Bun scored
471.06 at 111.5 MiB, and Node scored 456.95 at 172.6 MiB. These single rounds
cannot establish a change against the prior independent screen, which scored
Quench 4.35. This is not a score win; the remaining gap is concentrated in
the linked-entry lookup loop and its heap and call work. The raw reports and
limitations are documented in
`evidence/jit-jetstream3-2026-10-20/int32-identity-arm/README.md`.

## Direct Int32 identity calls

The compiled same-program call path now accepts an already-promoted Int32
identity artifact. It checks the single numeric argument against the artifact's
existing Int32 domain guard; a miss goes through the ordinary activation path.
This removes the callee frame only for the effect-free identity arm.

On the cloud Linux x86_64 host, separate single-round `hash-map` screens
validated at 1.94 Score / 75.3 MiB for the changed binary and 0.91 / 75.2 MiB
for its parent binary. Quench wall time was 277.44 s and 432.29 s respectively.
The Bun and Node screens varied substantially between rounds, and the samples
were not paired, so this is directional evidence only. Quench still loses the
fixture's Score bar by over two orders of magnitude. The focused runtime test
type-checks but its test executable build was stopped after 29 minutes without
finishing. Full details are in
`evidence/jit-jetstream3-2026-10-20/int32-direct-call/README.md`.

## Web-SSR native coverage gap

The one-round Web-SSR screen on the same host scored 6.06 in JIT mode and 5.77
in interpreter mode, with peak RSS of 135.5 MiB and 133.1 MiB. Node scored
92.42 / 203.6 MiB and Bun 81.45 / 227.8 MiB. The diagnostic run compiled only
the benchmark driver's `quickHash`; the hot generated React functions `Ru` and
`Eu` stayed interpreted. `Ru` has 831 bytecodes, beyond the current 256-bytecode
candidate limit, while `Eu` uses captures and calls the current JS tier does not
lower. These screens have one round each and do not qualify a win. See
`evidence/jit-jetstream3-2026-10-20/web-ssr-baseline/README.md`.

The guard-census follow-up attributes the same coverage gap more precisely:
`Ru` entered the interpreter 15,525 times with 831 bytecodes, 47 calls, 42
capture loads, allocations, and throws; `Eu` entered 15,524 times with 53
bytecodes, 3 captures, a call, and exception-capable control flow. `Ru` is
over the bytecode budget, while `Eu` fails activation eligibility before
lowering. Their 49 and 2 conditional branch sites are census counts, not proof
of redundant guards. Current evidence therefore gives no sound small pure
predicate region for ISOP or lazy block versioning. Keep the existing tier
architecture and exact interpreter fallback; next useful work is to improve
native coverage of the renderer's effectful call/field path or find a smaller
hot subregion with measured payback. The diagnostic report is from one
instrumented round and is not performance evidence; details are in
`evidence/jit-jetstream3-2026-10-20/guard-census-attribution/`.

## Cached `this` field returns

The JIT-mode entry has a narrow direct-getter matcher for a return-marked
`GetField` from fixed `this`, followed by the compiler's unreachable fallback
tail. On a cached data-property hit, it returns the value through an ordinary
`Return` opcode. Accessors, proxies, cache misses, and non-fixed `this` use the
interpreter from bytecode zero. Diagnostic counters confirmed 10,799,999 fast
entries each for `get key` and `get value`, out of 10.8 million entries per
function; the one initial cache miss per function stayed interpreted. The
JetStream round validated output. One corrected-matcher production round
scored 3.76 at 74.13 MiB; Bun scored 245.20 and Node 136.06. That sample and
the saved parent sample (1.02 at 73.82 MiB) are unpaired, and the peer scores
varied substantially between runs. They do not establish an optimization or a
win. The earlier one-round score delta used a matcher that had zero fast
entries and is noise. Tightening the candidate to the observed four-word
shape produced a similar one-round sample (3.75 / 74.06 MiB); this also remains
unpaired evidence. Corrected diagnostic data and screen reports are in
`evidence/jit-jetstream3-2026-10-20/this-field-return-screen/`.

The matcher now also accepts a three-bytecode return-marked `GetField(this)`
getter with its unreachable fallback tail. In `sync-fs`, this covers
`File.get data()`: one diagnostic counted 36,951,119 guarded returns from
36,951,120 entries, with the first cache miss interpreted. Across three clean
production samples, the candidate median Score was 2.86 versus 2.59 for its
parent control, while median peak RSS stayed near 72.2 MiB for both. Median
Quench wall time fell from 123.38 s to 112.25 s. The first two samples were
grouped by binary and the peer scores moved substantially, so this is a small
directional screen, not qualification. Quench still trails Bun and Node by a
wide margin. Details and reports are in
`evidence/jit-jetstream3-2026-10-21/sync-fs-getter/`.

The `async-fs` memory profile exposed a separate host allocation problem:
`NodeHost` created a VM `WeakRef` and host weak root for each Promise even when
no async-hooks `init` or `destroy` callback was enabled. I kept Promise IDs and
job-context propagation intact, while omitting that weakly tracked lifecycle
record when neither hook kind has listeners. An uninstrumented production one-round JIT
screen validated output and reported 3.44 Score / 891 MiB RSS for Quench, versus
73.60 / 210 MiB for Node and 128.96 / 67 MiB for Bun. The earlier Quench
production one-round screen was 1.31 / 7.26 GiB. This is a large memory
reduction but still loses on both metrics; the samples are diagnostic, not a
paired qualification. Raw reports and the reproduction command are in
`evidence/jit-jetstream3-2026-10-21/async-fs-promise-lifecycle/`.

The async-fs diagnostic also found 259,198 hot `CountVisitor` method entries
that update one own numeric field and return `undefined`. I added a guarded
JIT-mode fast entry for that exact bytecode shape; it uses the existing
successful own-field load/store caches, and only skips interpretation for a
fixed receiver with a Number value. The iteration diagnostic confirmed 59,119
fast `visitFile` returns and 200,079 fast `visitDir` returns, with one cold
interpreter entry per method. A seven-sample microbenchmark measured median
57.86 ms for one million guarded calls versus 193.68 ms for the interpreted
shape. The production async-fs screen scored 3.09 at 891 MiB; one earlier
screen scored 3.44 at the same RSS, with substantial peer-score variation.
This confirms a local speed path but not a JetStream Score gain. Quench remains
well behind Node and Bun. Details and reports are in
`evidence/jit-jetstream3-2026-10-21/async-fs-field-increment/`.

## Full JetStream 3 coverage screen

A one-round screen of the complete suite produced valid matching output on 33
of 34 fixtures; `argon2-wasm` hit the 600-second per-engine timeout for
Quench. Across the valid fixtures, Quench's geometric mean score was 8.46% of
Node and 5.81% of Bun. The largest valid score gaps were `sqlite3-wasm`,
`espree-wtb`, `typescript-lib`, and `async-fs`. Peak RSS also remains a major
problem on `doxbee-promise` (966 MiB), `doxbee-async` (1004 MiB), and
`async-fs` (891 MiB), while Quench uses less memory than peers on several
other workloads. These single rounds are diagnostic screens, not
qualification.

A three-round glibc allocator screen tried `MALLOC_TRIM_THRESHOLD_=0` on
`doxbee-promise` and `doxbee-async`. Median peak RSS was unchanged (within
0.01%) in both fixtures. Quench Score rose 3.8–4.3%, but large peer timing
drift between sequential screens makes that inconclusive. Aggressive trim
is not retained as a runtime setting. Raw reports are in
`evidence/jit-jetstream3-2026-10-21/doxbee-glibc-trim/`.

The focused `typescript-lib` profile counted 28.9 million function entries,
3.93 million native entries, 89 promotions, and 43 successful native
compilations. The highest-cost work is in closure/call-heavy functions and
functions above the current 256-instruction limit. The static guard census
counts branch and type-check sites, but these profiles do not show meaningful
redundant-guard coverage. This points to execution coverage and dynamic call,
closure, and object costs as higher priorities than ISOP or other Boolean
rewriting. Keep ISOP as a bounded offline experiment until a matched fixture
shows an actual score gain. Full reports and reproduction commands are in
`evidence/jit-jetstream3-2026-10-21/full-suite-screen/`.

## SQLite Wasm execution profile

The `sqlite3-wasm` diagnostic scored 0.25 / 162.4 MiB RSS for Quench, versus
25.96 / 67.6 MiB for Node and 12.26 / 89.4 MiB for Bun. This agrees with the
full-suite screen's 0.27 score. The instrumented workload compiled 2,517 of
2,533 functions in its largest module, emitting 8.04 MB of code in 24.7 s;
across module instances it recorded 23,033 native functions, 28.0 MB of code,
69.7 s cumulative compilation, 26.0 million native entry requests, and 2.23
million compiled-to-VM reference callouts. It reports zero compiled import
callouts. This points toward generated-code throughput, module compilation,
and reference-call boundaries as the next Wasm-side diagnostics. It does not
justify changing the existing tier or cache policy without a matched A/B.
Report and reproduction details are in
`evidence/jit-jetstream3-2026-10-21/sqlite-wasm-screen/`.

The same-artifact reference-body cache was then screened on SQLite in three
paired rounds. Cache-on scored 0.30 versus 0.32 off (paired ratio 0.897,
95% interval 0.841–0.938); RSS fell only 0.38% (ratio 0.9962,
0.9928–0.9985). The cache worsened score and remains opt-in. This was a
diagnostic screen with a dirty-source provenance flag caused by widening the
bench runner's fixture validation; both arms used the same production runtime
binary. The runner now permits this matched screen for either `tsf-wasm` or
`sqlite3-wasm`. Reports are in
`evidence/jit-jetstream3-2026-10-21/sqlite-reference-body-cache/`.

## Numeric hash-map entry loop

The observed 23-instruction `_findKeyEntry` loop has an opt-in bounded JIT-mode
entry path for numeric keys (`QUENCH_JS_JIT_NUMERIC_FIND=1`). It checks cached
own fields, supports present dense and sparse array data, resolves only
owned/shared lexical captures, and uses an already-compiled same-program
numeric `equals`. It returns through the ordinary bytecode `Return` and leaves
the frame untouched on every failed guard. The path polls interrupts and falls
back after 256 chain entries.

A one-round diagnostic validated the fixture and counted 53,999,968 fast
returns out of 54 million entries; the first cache miss used the interpreter.
A clean one-round screen scored 3.61 at 74.07 MiB and 125.32 seconds, versus
Bun 108.18 / 99.03 MiB / 1.75 seconds and Node 124.44 / 159.59 MiB / 1.79
seconds. Quench used less RSS, but the score remains far behind and the single
round is not a matched comparison. This is coverage evidence, not a benchmark
win. Reports are in
`evidence/jit-jetstream3-2026-10-21/numeric-find-loop/`.

The matcher now caches its last positive program/function result, avoiding a
bytecode rescan at every hot entry. A later one-round screen was directionally
faster (4.52 / 74.17 MiB / 111.36 s), but Node and Bun scores moved from 124.44
and 108.18 to 155.13 and 175.49 as well. The screens are unpaired and do not
establish a score win.

An interleaved same-binary three-pair screen compared the path enabled and
disabled. Disabled was faster in every pair (105.48 vs 126.62 s, 110.01 vs
117.55 s, and 114.85 vs 127.91 s) with effectively unchanged RSS. The
high-coverage loop therefore stays opt-in rather than shipping as the default.

## Promise async-hooks context retention

An allocation trace found a ~544 MiB async-hooks trigger map on Doxbee
Promise workloads. Promise trigger contexts were retained after their Promise
objects became unreachable. Trunk already gives a promise no context while
nothing tracks promises (`create_promise_context`). For a tracked promise, the
sweep that frees it now hands its context to `Host::release_promise_contexts`,
apart from the reaction-job tokens it hands to `Host::release_job_context` (the
host numbers the two independently), and the Node host drops its trigger entry;
no collection walks the live promises. The measurement below was of the earlier
v2-jit form of this fix, which collected the live promise contexts after each
full collection.

An uninstrumented three-round JetStream 3 run validated output and reduced
median Quench peak RSS from 1,012 MB to 170 MB on `doxbee-promise`, and from
1,053 MB to 258 MB on `doxbee-async`. Scores were 4.35 and 5.83. The older
scores were 3.69 and 4.69, but sequential peer drift means the score changes
are inconclusive; Quench remains much slower than Node and Bun. Raw data and
method are in
`evidence/jit-jetstream3-2026-10-21/async-hooks-promise-context-cleanup/`.

A one-round Doxbee Promise JIT census found zero native entries across 10.8
million function entries. The hottest `doxbee` and anonymous closure bodies
were rejected as unsupported activation/function kinds; `runIteration` made
one compile attempt but its loop includes calls, array mutation, and Promise
aggregation. The measured branch/type-check sites do not establish redundant
guards, so Boolean simplification is not the next target. Census data is in
`evidence/jit-jetstream3-2026-10-21/doxbee-native-entry-census/`.

A fast return for empty AsyncLocalStorage maps was screened in sequential
three-round builds. Promise score moved upward while Async moved downward, with
flat RSS and large peer timing drift. The change was reverted because the
screen did not establish a matched win. Results are in
`evidence/jit-jetstream3-2026-10-21/doxbee-empty-als-snapshot/`.

The one-round `typescript-lib` diagnostic reinforces the same coverage issue:
Quench scored 0.05 versus Node 1.45 and Bun 1.12, with 567 MiB versus about
359 MiB peak RSS for both peers. Of 2,348 function rows, 2,178 were unsupported;
the largest buckets were unsupported activation/function kinds (1,228 rows),
unsupported opcodes/control flow (713), and the 256-instruction limit (117).
The hottest rows include `forEach`, `checkSourceElement`, `findSourceFile`, and
`bind`; these are interpreter-heavy loops and closure/call paths. The profile
counted 28.9M entries but only 3.93M native entries, with 43 successful
compilations. These are diagnostic results from one round. The raw profile is
`evidence/jit-jetstream3-2026-10-21/full-suite-screen/typescript-lib-diagnostic-1round.json`.

Increasing the instruction cap from 256 to 512 compiled two additional
TypeScript helpers and raised native-entry counts by about 10%, but left the
one-round score at 0.05. In three rounds the 512-cap screen scored 0.07 /
565 MiB versus 0.08 / 564 MiB for the 256-cap binary. The cap increase was
reverted: this additional native coverage did not improve score or RSS.
Reports are in
`evidence/jit-jetstream3-2026-10-21/typescript-jit-instruction-cap/`.

A faster Fx hasher for internal Promise async-ID and job-token maps was also
screened. Promise score rose from 3.07 to 3.60 while Async fell from 5.19 to
3.22; only Async RSS improved (2.3%), and peer timings varied substantially.
The hasher change was reverted because these sequential screens did not show a
consistent score and memory win. Reports are in
`evidence/jit-jetstream3-2026-10-21/doxbee-fxhash-context-map/`.

## Guarded numeric hash-map lookup

`_findKeyEntry` has an exact bytecode matcher for its 23-instruction linked
entry loop. The fast helper is guarded at function entry, uses the existing
field caches for the list and entry fields, and calls the already-compiled
numeric `equals` target. On a guard miss, it leaves the frame untouched for
ordinary interpretation. A 3-pair same-binary JetStream `hash-map` screen
showed enabled median Score 2.93 versus 2.63 disabled, with sampled peak RSS
74.17 MiB versus 74.06 MiB and median wall time 163.9 s versus 174.6 s. All
three adjacent score pairs favored enabled. The helper is now enabled by
default for eligible inputs; `QUENCH_JS_JIT_NUMERIC_FIND=0` retains the
interpreter for A/B screens. The result is a fixture-specific screen, not
qualification against Node/Bun or a suite-wide win. Raw reports and method
are in `evidence/jit-jetstream3-2026-10-21/hash-map-numeric-find-screen/`.

A separate matched three-round peer screen on this fixture measured Quench at
2.83 Score / 73.99 MiB, Bun 1.4.3 at 165.97 / 83.56 MiB, and Node 24.19.0 at
124.87 / 222.77 MiB. The numeric lookup change preserves the RSS advantage
but does not close the score gap (Quench reaches 1.7% of Bun and 2.3% of
Node). The next target is the interpreted `run` body: 58 instructions, 120
entries and 111.7 seconds inclusive in the opt-in diagnostic, containing
allocation, throwing, call and loop behavior. Boolean guard rewriting is not
justified by its census. The report is in
`evidence/jit-jetstream3-2026-10-21/hash-map-numeric-find-screen/peers-3round.json`.

A second, twelve-round A/B tried caching and directly invoking the compiled
Boolean `equals` entry inside `_findKeyEntry`, while retaining the code mapping
owner. The raw entry lost overall: median Score 2.385 vs 2.515 through the
existing `js_jit_compiled_call`, median wall time 174.9 vs 169.0 s, and RSS
74.23 vs 74.19 MiB; only 3/6 adjacent score pairs favored it. The code path
was reverted. This points away from per-equality dispatch lookup as the main
score limit; next focus remains the interpreted `run` loop and its calls,
allocations and collection behavior. The six-pair raw data are stored in
`evidence/jit-jetstream3-2026-10-21/hash-map-numeric-find-screen/`.

### Hash-map diagnostic attribution

Reviewing the existing opt-in diagnostic report clarified the timing scope:
`runIteration` has 32 entries and 175.968 seconds of inclusive interpreter
time, while its child `run` has 120 entries and 111.730 seconds. These times
overlap and must not be added. `computeHashCode` enters native code for all
64.8 million recorded entries, and `equals` for 53,999,969 of 54 million.
The numeric lookup helper takes its guarded fast path 53,999,968 times. Its
3 ms `interpreter_inclusive_ns` value covers only ordinary interpreter
completion and excludes that helper's native execution, so it is not a cost
estimate for the lookup path. The evidence points to the effectful outer
`run`/`runIteration` dispatch as the remaining measured opportunity; it does
not justify a Boolean guard rewrite or another equality-call shortcut. Raw
diagnostic source: `enabled-diagnostic-1round.json` in the same report
directory. This is attribution from instrumented inclusive counters, not an
exclusive CPU profile or an optimization result.

The census initially missed functions that ran as lane activations because it
counted only ordinary JIT frame entries. Opt-in diagnostics now report
`lane_entries` separately and include them in `all_entry_native_coverage_ppm`.
This exposes `get`, `_getEntry` and `put` as lane-only entries in the hash-map
fixture, while preserving frame-entry coverage for native JIT functions. A
follow-up tried invoking compiled numeric callees directly at lane call sites.
The same-machine three-round A/B was a loss: control median 3.22 Score / 74.01
MiB versus candidate 2.87 / 78.28 MiB, with all rounds output-equivalent. The
fast path was removed. The census showed those callers already pass through
native `computeHashCode` and `equals` entries, so this dispatcher shortcut was
redundant and increased peak RSS. Reports and full per-round data are in
`evidence/jit-jetstream3-2026-10-21/hash-map-lane-census/`.

A second narrow experiment matched the Number/Number branch of the benchmark's
`equals` bytecode and replaced its call with direct `f64` equality after both
Number guards. The diagnostic confirmed 53,999,999 fast comparisons, but a
same-binary three-round screen scored 4.09 versus 4.57 through the compiled
`equals` call; RSS improved by 0.46 MiB and only 1/3 adjacent score pairs
favored the direct comparison. It was removed because the score regressed.
The original call remains the fallback and active path. Raw diagnostic and
paired data are in the same evidence directory.

### Async-fs Float64 DataView calls

The JetStream `async-fs` fixture spends substantial time in `File.swapByteOrder`
calling `DataView.getFloat64` and `setFloat64`. The lane fast path handles only
primitive numeric offsets and values on fixed, ordinary, non-shared DataViews
in compiled (`--jit`) mode; interpreted mode stays on its existing path. All
coercion, bounds-error, detached, resizable, shared and unsupported buffer cases
remain on the generic native implementation. The opt-in diagnostic counted
8,204,080 fast gets and 8,204,080 fast sets in the fixture.

The three-round same-binary screen scored 2.97 with the path disabled and 3.32
enabled (11.8% higher median), with peak RSS effectively unchanged at 87.93
and 87.92 MiB. Five interleaved one-round pairs favored the path in four of
five pairs; the median paired Quench/Node score ratio improved 9.5%, which
controls for substantial host variation visible in the Node and Bun scores.
The feature remains far behind the JIT peers on this fixture: the same
three-round screen measured Bun at 110.07 Score / 66.63 MiB and Node at 50.42 /
206.71 MiB. The change is retained as a local win, not a JetStream victory.
Reports and pair-level detail are in
`evidence/jit-jetstream3-2026-10-21/async-fs-dataview-fast-lane/`.

The follow-up fastens the common Boolean endianness argument check in this
lane: Boolean arguments use `Value::as_bool`, while all other values retain
general `ToBoolean` handling. Three-round control/candidate medians were 3.80
and 3.94 Score, with 87.75 and 88.04 MiB peak RSS. In seven interleaved
one-round Quench pairs, the candidate won six; the paired median and geometric
mean Score ratios were 1.083 and 1.106, and median RSS ratio was 1.0014.
Candidate-first pairs were added to check run-order bias. Peer scores varied
sharply, so this supports a local async-fs improvement only; it is not a
JetStream peer win or qualification. Reports and methodology are in
`evidence/jit-jetstream3-2026-10-22/async-fs-dataview-boolean-fast-path/`.

A follow-up removed the unused body-pointer word from each Wasm
indirect-call-site certificate in default mode, moving it to a separate array
only for the opt-in body-cache experiment. The SQLite Wasm three-round screen
validated both executables but did not show an RSS reduction (170,295,296 B
control; 170,586,112 B candidate). The candidate patch was dropped because the
8-byte-per-site saving was below useful peak-RSS resolution. Full reports are
in `evidence/jit-jetstream3-2026-10-22/sqlite-reference-cache-compact/`.

The async-fs native call dispatcher also previously read the same callee cell
to check for `Function.prototype.call` and again to classify the native. A
candidate now classifies the native once and reuses that value for all fast
handlers, retaining generic fallback for other callees. Six interleaved
one-round pairs all validated output. Median paired Quench Score ratio was
1.073 (geometric mean 1.117), while Quench peak-RSS deltas had a median of
zero. The same-run Bun/Node normalization had a 0.974 geometric mean due to
large peer variation, so the sample does not establish a causal score or RSS
win. Keep interpreting it as a low-risk local candidate while broader JIT
coverage remains the larger opportunity. Reports are in
`evidence/jit-jetstream3-2026-10-22/async-fs-native-dispatch-dedup/`.

## V8-v7 native coverage census — 2026-10-22

Current `QUENCH_JS_JIT_STATS=1` runs show native function entries on Crypto
(3,195) and DeltaBlue (35,055 across three tiny functions); OSR executes
4,730,536 regions on Crypto and 48,954 on NavierStokes. Richards has zero
native entries and zero admitted OSR sites: its hot `runRichards` and
`Scheduler` functions reject on unsupported shapes. Its scheduling workload
uses mutable object fields and indirect calls, so the next broad compiler
coverage work should prioritize guarded field access and cached calls with
exact instruction-level interpreter fallback. This is coverage evidence, not
a performance claim; the diagnostic runs overlapped release-feature builds,
so their Score and wall-time outputs are excluded. Raw inputs/counters and
method are in `evidence/jit-jetstream3-2026-10-22/v8-v7-native-coverage/`.

### Recheck after the merged OSR tier — 2026-10-22

I rebuilt the production binary from the post-merge source and reran the same
four stock V8-v7 scripts with `QUENCH_JS_JIT_STATS=1`. The runs completed their
benchmark assertions; their printed Scores are not comparable performance
measurements. Richards still has zero native function entries and zero admitted
OSR sites. DeltaBlue has 17,539 native function entries and no admitted OSR
sites. Crypto has 2,456 native function entries, 12 admitted OSR sites, 8 OSR
compilation successes and 6,120,293 region runs. NavierStokes has no native
function entries, 22 admitted OSR sites, 26 reported OSR compilation successes
and 17,435 region runs. The merged OSR mode therefore adds loop execution for
numeric regions, but does not yet cover Richards' field-, call-, construction-
and write-heavy hot loops. This is diagnostic coverage evidence only; it does
not establish a score or memory win. Full logs and binary provenance are in
`evidence/jit-jetstream3-2026-10-22/v8-v7-coverage-after-osr-merge/`.

## Bounded `new Array(number)` fast path screen — 2026-10-22

A lane fast path plus direct dense-hole initialization looked promising in an
initial Richards pair, but that control binary predated the source merge and
was not comparable. Rebuilding the baseline at the candidate's exact base
revision reversed the result: across 11 valid fixed-work rounds, outputs were
identical, but candidate marginal work time was 5.9% slower by paired geometric
mean (1.059 candidate/baseline), faster in only 2/11 rounds, and median RSS was
effectively unchanged (12,066,816 vs 12,034,048 bytes). The experiment was
removed. The runner could not collect IPC/cycle counters, so this remains a
diagnostic screen; it does not measure JIT mode or qualify JetStream. Raw report
and provenance are in
`evidence/jit-jetstream3-2026-10-22/array-length-construction-ab/`.

The next performance work should target suite-scale bottlenecks. Current JIT
coverage is absent on Richards and sparse on TypeScript, so extend the shared
compiler with general guarded field and inline-cache call denotations, then
measure OSR coverage, compilation cost, code size and paired fixture scores.
For no-JIT, old per-op instruction screens show large dispatch and allocation
costs in object/closure creation, field/array operations and string/RegExp
builtins; these need current profiles and fixture-level validation before
choosing a hot path. No 20% runtime-level gain has been demonstrated yet.

## Guarded multi-field numeric inputs — 2026-10-22

The numeric denotation now accepts up to four cached reads from `this` or
parameter zero in pure numeric regions. Each field cache is checked at entry;
only numeric values enter native code, and any miss resumes the untouched frame
in the lane. This is general bytecode metadata, not a function/source matcher.
All 799 runtime library tests pass. The functional unit test enters native code
for a two-field `this` method and confirms accessor and non-number fallback.

The V8-v7 Richards diagnostic still reports zero native entries for
`isHeldOrSuspended` (32 entries); the remaining unsupported combination is two
field reads, numeric captures and a branch. One-round JetStream 3
`typescript-lib` and `async-fs` diagnostic runs timed out at the 600-second
per-engine limit before valid samples, so no score or RSS gain is established.
The step is recorded as compiler coverage groundwork, not a performance win.
Evidence and raw reports are in
`evidence/jit-this-field-inputs-2026-10-22/`.

## Research convergence and next gates — 2026-10-22

The Linear sweep and VM research review agree on the next JIT gate: measure
compiled coverage and rejection causes by time-weighted hot region, then extend
general bytecode denotations for the operations those regions actually use.
The current V8-v7 census has no native entries on Richards; the hash-map hot
wrapper rejects `LoadNameCall` and `Call`, even though helper functions already
receive many native entries. General effect-aware call and field/write paths
with exact instruction-PC fallback are more promising than adding source or
function recognizers. Retain native code only when matched suite measurements
show a gain within code-size and RSS budgets.

Research patterns that fit the shared-semantics architecture:

- SpiderMonkey's [CacheIR](https://firefox-source-docs.mozilla.org/js/cacheir.html)
  describes guarded operations once and lets execution tiers consume the same
  recipes. For Quench, Rust macros should generate the lane and compiler views
  from one semantic authority; effectful recipes must preserve observable order
  and the exact fallback frame.
- V8's [Maglev](https://v8.dev/blog/maglev) tracks type facts during graph
  construction and records interpreter state at deoptimization points. This is
  a useful model for eliminating checks only when prior guards prove the facts.
- [Basic-block versioning](https://doi.org/10.4230/LIPIcs.ECOOP.2015.101) can
  reuse type/shape facts across predecessor edges. It is a follow-up only if a
  guard census shows repeated checks; cap per-block versions and generated
  bytes, and invalidate facts across effectful calls and writes.
- The 2025 [inline-cache evaluation](https://doi.org/10.22152/programming-journal.org/2025/10/6)
  reports that fewer memory accesses did not imply faster execution in its
  tested JavaScript AOT setting. It reinforces measuring target code and suite
  score, rather than treating guard or lookup counts as a speed proxy.

For no-JIT, TOD-121's measured plan remains around 0.656x Bun geomean and its
grounded queued work reaches only about 0.74x. The `typescript-lib` census
reports 10.5M+ property lookups per iteration at the shared uncached site after
u16 site exhaustion. Widening or segmenting site IDs is a bounded experiment,
but likely affects only a narrow workload slice. First run the existing
`profile-lane-exits` census across parser, async and object-heavy workloads;
then prioritize costs by total dispatched time. Large string/allocation
representation changes remain research hypotheses until those profiles size
their opportunity and the semantic costs.

TOD-164's newer comments narrow another no-JIT lead. Survivor-only record
sweeps and young-word-only minor sweeps are already merged; the latter measured
+2.7% on a five-pair GC-heavy JetStream screen, about -0.3% on V8-v7, and
roughly neutral RSS. Stable-root/frame-watermark skipping was within noise and
is deprioritized. `doxbee-promise` still merits attribution: about 20% of its
measured callgrind instructions were in GC over four sampled iterations, but
its 10,000 live promise chains may require most of that marking work. Before
proposing another collector optimization, split minor/full collection counts,
newly allocated and live young records, owner scans, and mark work per pending
job. These measurements identify where to look; they do not establish another
broad speedup.

JetStream 3's [published methodology](https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/)
uses repeated per-subtest measurements and suite aggregation. Papers and
diagnostic counters above are leads only: neither the 1.20x score goal nor a
20% broad no-JIT/JIT win is established by this research.

## Effectful JS call-region ABI boundary

The current JS native entry is deliberately primitive-valued; it cannot carry
arbitrary tagged JS values or keep guest heap references live across collection.
The first compiled call region therefore needs a VM-owned continuation context,
not a wider raw-pointer callback. Before entering the shared dispatcher, write
all live native values back as tagged values in GC-rooted frame registers and
publish the exact instruction PC, resume state, and binding-site PC. The
callback must reacquire VM/frame data by a stable frame identity after dispatch;
it must not retain Rust references or slices into VM storage across a callout.

The callback uses the existing instruction dispatcher as the semantic
authority. It encodes the full `StepResult` and error outcome. Native execution
may resume only when dispatch completed normally with `Continue`, the same
program/function/frame remains active, and the continuation state matches the
compiled map. `Return`, `Throw`, `PushFrame`, `TailCall`, yield, panic, and any
other non-Continue result leave native code for the existing VM. A consumed call
must never be retried at the old PC. Catch Rust panics inside any C ABI callback;
no unwind may cross generated-code boundaries. Drop heap-derived type/shape
facts across effectful calls and writes.

Start with ordinary nonterminal `Call` over already-resolved callees, excluding
direct or parameter eval, spread, tail calls, and terminal-return cases. Keep
`LoadNameCall`, `with`, getters/proxies, constructors, and other effectful name
resolution on the current fallback until their exact receiver, lookup, and
error behavior is represented. This restriction is a pilot boundary, not a
source/function recognizer: admission is derived from bytecode effects and
explicit flags. Required tests cover receiver and argument order, non-callable
and throwing callees, reentrant calls that collect while values are live,
callee replacement, and exactly-once effects with fallback at the failing PC.

This protocol is correctness and coverage groundwork. `runIteration` can check
the ABI and output parity, but its observed call count does not make it a score
target. Before choosing a hot site, record time-weighted compile rejection
reasons, call-target/cache state, receiver-shape diversity, entries and
callouts. A cache/guard hit is useful only if it reduces measured VM work; keep
it only on matched production JetStream score and RSS data.
