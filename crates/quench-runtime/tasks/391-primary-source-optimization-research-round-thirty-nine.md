# 391 — Primary-source optimization research, round thirty-nine

Status: complete

Research current primary sources for general algorithms that can improve this VM while
preserving its fixed constraints: OXC plus Rust, no third-party VM, rustc/LLVM cooking at
build time, copy-and-patch at load time, stencil/kernel execution from the first call,
no execution-count hotness gate, and no V8v7-shaped selector.

Deegen's chronology is stated precisely here: the arXiv preprint was first submitted on
18 November 2024; the archival PACMPL/OOPSLA publication is dated April 2026. The design
must not be called a "2026 design" as though it originated then.

## Findings

### 1. Record only feedback that has a consumer

A 2026 characterization of type feedback reports recording overhead up to 1.6x, with a
1.2x mean, while at least 59% of non-empty slots were unused by the optimizing compiler.
Dead code accounts for much of the waste; many remaining observations merely repeat a
type already inferred statically. Those measurements are for the R compiler and are not
performance predictions for this JavaScript VM. The transferable algorithm is stronger
than compacting `InlineSite`: derive a `FeedbackDemand` set from the final quoted plan,
after dead-code elimination and recipe selection, and allocate/update only demanded facts.

This project does not have interpreter warmup, but direct stencils and slow IC arms can
still write mutable site data. Task 392 therefore makes the feedback plane a projection
of actual consumers. Runtime observations may select a semantic IC case on first use;
they may never count executions or decide that a function is hot.

### 2. Instrument the code that actually executes

Current diagnostic images disable direct blocks so that counters cannot be skipped. That
makes the profile observe a different execution engine. Copy-and-patch instrumentation
research shows the natural alternative: inject analysis stencils into the bytecode/code
stream at link time. In categorical terms an observation is an effectful endomorphism
`Observe<E, Gamma>: Gamma -> Gamma`; in a release build the observation functor maps it to
the identity. Task 393 adds separately linked diagnostic images whose ordinary stencils,
connectors, IC arms, and call continuations remain unchanged.

This is now a P0 measurement dependency. The engine cannot reliably choose region work by
native reach while enabling diagnostics turns the direct path off.

### 3. Let rustc/LLVM teach the finite stencil catalog offline

JavART extracts translation rules offline from an existing optimizing compiler and uses
the rules in a lightweight runtime compiler. The reported Android/HotSpot results do not
transfer to this VM, and this project must not add a learned runtime selector. The useful
pattern is an offline oracle: enumerate bounded, typed semantic micro-op forms; compile
whole forms with pinned rustc/LLVM; retain relocation-closed Pareto winners; derive the
immutable matcher/cost catalog; and validate each rule against generic semantics.

Task 394 applies that pattern without machine learning, V8v7 training, runtime LLVM, or a
second semantic definition. This turns rustc/LLVM-driven design into a repeatable catalog
construction process rather than manually guessing that a fused Rust function will lower
well.

### 4. A typed dynamic-language IR needs facts that runtime inspection cannot recover

The 2026 typed-IR work makes type concreteness, ownership, reflection, function versions,
and contextual dispatch explicit. In particular, a dispatch signature must carry static
facts such as ownership that cannot be reconstructed by inspecting runtime values. This
refines Tasks 163 and 171: `Context` must distinguish proven/static facts from guarded or
runtime-recoverable facts, and unknown calls, `eval`, `with`, proxies, accessors, and
observable identity must conservatively contribute explicit reflection/effect capability.
No second IR task is needed.

### 5. Use one cache-friendly CFG and forward copying reducers

V8's Turboshaft account reports that its old Sea-of-Nodes optimizer revisited nodes often,
made stateful analyses difficult, and had materially worse compiler-cache behavior than
the new CFG pipeline. JavaScript's many guarded/effectful operations usually constrain
work to basic blocks anyway. Task 171 already selected a compact CFG and immutable copying
reducers; the new evidence sharpens its storage and traversal acceptance criteria instead
of justifying an e-graph or Sea-of-Nodes detour on the critical path.

### 6. Choose inlining as a portfolio, not one call site at a time

WebKit replaced greedy per-call-site Wasm inlining with a non-local function-wide decision
that ranks all candidates by execution importance, callee size, and optimization unlocked,
then spends one code-size budget. This VM cannot use runtime frequency as a selection
input, but the portfolio algorithm still applies with static loop depth, exact target and
context facts, removed call/frame seams, newly closed native regions, predicted spills,
and copied code bytes. This belongs in Task 20's existing SCC-bottom-up inliner.

### 7. Do not repeat already represented mechanisms

- CPython's current copy-and-patch builder chooses size-oriented compilation because
  standalone-function tail duplication and alignment can hurt concatenated snippets.
  Task 273 already tested this exact question and retained O2 because local `Os`/`Oz`
  artifacts violated relocation closure.
- CPython's two-pass size/layout/fixup publication and per-image trampoline/GOT union are
  already Tasks 274, 348, 367, and 390.
- WebKit's contiguous object payload, inline bump allocation, and embedded type identity
  are already Tasks 191, 299, 321, and 369.
- CoSSJIT's static speculative-condition approach refines Tasks 144, 164, and 176; it does
  not require another speculation representation.
- The negative dynamic-binary-modification IC result reinforces Task 331: shortening a
  dependency chain matters more than reducing an isolated load count.

## Work created or refined

1. Task 393: composable instrumentation stencils for honest native-path measurement.
2. Task 392: demand-projected feedback plane, measured using Task 393.
3. Task 394: offline rustc/LLVM-oracle translation-rule synthesis.
4. Task 20: non-local budgeted inlining portfolio over its existing SCC candidate graph.
5. Task 171: static-versus-runtime fact provenance, reflection capability, compact arena
   storage, and forward copying-reducer metrics.
6. Tasks 157 and 166: consume the offline rule catalog and feedback-demand census rather
   than inventing independent selectors or reports.

The priority remains architectural: finish Task 390's patch algebra, make instrumentation
honest with Task 393, then use measured physical closure to finish the register-resident
region and direct-call continuums. Tasks 392 and 394 remove systemic overhead and manual
catalog guesswork, but neither substitutes for Tasks 385, 20/145/379, or total native
coverage.

## Primary sources

- Deegen arXiv record, first submitted 18 November 2024:
  <https://arxiv.org/abs/2411.11469>
- Deegen archival paper, PACMPL/OOPSLA publication dated April 2026:
  <https://doi.org/10.1145/3798246>
- Characterizing Type Feedback in Just-In-Time Compilation:
  <https://doi.org/10.4230/LIPIcs.ECOOP.2026.16>
- A Typed Intermediate Representation for Dynamic Languages:
  <https://mlaurent.ovh/publications/typed_ir.pdf>
- Leveraging Copy-and-Patch JIT for Low-Overhead Dynamic Program Analysis:
  <https://doi.org/10.1145/3828170.3828176>
- JavART rule-guided lightweight compilation:
  <https://doi.org/10.1145/3720418>
- V8's cache-friendly CFG/copying-reducer rationale:
  <https://v8.dev/blog/leaving-the-sea-of-nodes>
- WebKit's non-local inlining, inline allocation, and contiguous-layout account:
  <https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/>
- CPython's current stencil builder and linker:
  <https://github.com/python/cpython/blob/main/Tools/jit/_targets.py> and
  <https://github.com/python/cpython/blob/main/Python/jit.c>
- False lead of optimizing inline caches:
  <https://arxiv.org/abs/2502.20547>
- CoSSJIT static analysis plus speculative conditions:
  <https://doi.org/10.1145/3763149>

