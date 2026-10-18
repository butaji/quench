# Quench JIT design

## Goal

Make JIT mode in `quench-node` beat JIT-enabled Node and Bun on JetStream 3.0 Score while using less maximum RSS, first on the server-side JavaScript fixtures. Keep the interpreted mode as the default and preserve its results. Use one OXC frontend, one residual bytecode representation and the same `Vm`, heap and host APIs in both modes. TypeScript is erased by the existing frontend; Wasm keeps its current decoder and enters that same VM.

The pass condition is workload-specific and measured: stock JetStream validation, 11 or more matched rounds, and a paired 95% bootstrap interval showing Quench's score ratio above 1 and its maximum-RSS ratio below 1 against each available JIT-enabled bar. Qualify on Linux x86_64 and M4 separately. A single run is a screen, not a win. JetStream 3 contains both compute-heavy JavaScript and Wasm, so continue to its Wasm fixtures after the JavaScript tier has a useful measured path.

## What the repository already has

At the `v2-merged` baseline, `quench-runtime` already used Cranelift to compile a subset of Wasm functions through `ExecutionMode::Compiled`; JavaScript and TypeScript still used the residual bytecode interpreter, and `quench-node` had no process-level JIT selector. This branch adds `--jit` to the existing runtime path and a lazy JS function-entry tier. The Wasm compiler remains a reference for executable memory, traps and host callouts.

The first memory risk is the granularity of code ownership: compiling all eligible functions when one Wasm function is first called makes cold code consume both compiler work and executable bytes. The JavaScript tier should compile a function only after it is hot, account for emitted bytes, and stop compiling when a named per-process budget is full. Keep no IR, AST or copied bytecode alongside native code unless a correctness invariant needs it. Code-cache exhaustion means interpreted execution.

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

The current tier is a prototype, not a measured optimization strategy: its exact sequential gate estimates guard stability, but compilation cost, observed time saved and expected remaining calls are not yet included in the payback estimator above. Ershov/Sethi-Ullman scheduling is not yet applied because these functions are short and Cranelift handles native register allocation. The category-theory state relation is expressed through a shared comparison classifier, guard invariant, forward-DAG restriction, rooted return map, and ordinary dispatcher return; it is not a runtime type or compiler abstraction. The string-return tier rejects loops, joins, heap access, calls, exceptions and dynamic return values; all non-Number inputs and unsupported shapes remain in the interpreter. Native candidates are capped at 256 bytecodes and 256 registers; the compiled body has no internal poll, so the shared dispatcher checks cancellation before entering and immediately after returning. Profiling-feature builds leave this JS tier disabled so native execution cannot omit interpreter instrumentation. Unit tests count actual native calls in test builds; normal benchmark reports record mode selection; opt-in `--jit-diagnostics` adds candidate entries, profiles, promotions, compile time, mapped bytes, and native entries to captured stderr for attribution-only runs.

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

The root mode has limits of 32 selected functions, 80,000 bytecodes per
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
only that function, forces Cranelift `opt_level=none`, and permanently leaves
known callees in the VM. `--jit-wasm-root-first` starts from the same single
function artifact and compile level, then lets hot fallback callees use the
existing bounded promotion tier. The strict mode is useful for measuring the
cost of one native entry; root-first tests whether the selected root can benefit
from a smaller first compile while retaining tier-up. The CLI records separate
checkpoint/report fields and provenance for both, and neither mode is eligible
for qualification. Root-first promotion is enabled only if the selected root
compiles successfully; a rejected root leaves the workload interpreted. Their runtime switches are
`QUENCH_WASM_JIT_ROOT_ONLY=1` and `QUENCH_WASM_JIT_ROOT_FIRST=1`, respectively.

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

On the same clean source revision, an 11-round selective-mode screen improved
gcc-loops median Score from 57.11 to 71.28 (+24.8%) relative to default JIT;
the median first timed iteration fell from about 306 ms to 118 ms, while
average iteration time changed from about 33 ms to 35 ms. QuickSort Score was
roughly flat (183.47 default, 180.05 selective). Both modes retain lower RSS
than Bun and Node but remain below both peers on Score. The default paired
analysis has Score ratios of 0.320 [0.311, 0.351] for gcc-loops vs Bun and
0.474 [0.459, 0.514] for QuickSort vs Bun. Selective mode intentionally has
`qualification_ready: false`; its medians are a screen without a paired
qualification claim. Raw reports and diagnostic limits are documented in
`evidence/jit-jetstream3-wasm-2026-10-18/selective-callknown-handoff/`.

The selective one-round diagnostic reports 309,089 gcc-loops native-to-VM
callouts (309,039 imported), against 9,955 total for QuickSort. It reports
345.5 ms summed tier compilation time for gcc-loops and 39.7 ms for QuickSort.
The aggregate does not attribute individual import symbols. This separates
two next optimization problems: attribute and lower repeated imported-call
boundary costs for gcc-loops, and improve generated code or native-entry
overhead for QuickSort, whose hot sort path has few callouts. Selective tier-up
is only an admission mechanism; it does not by itself solve either runtime
cost and it does not satisfy the requested peer-score target.

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
