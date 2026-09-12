# 327 — Primary-source algorithm research, round twenty-five

Status: complete

Research additional VM/compiler algorithms under the standing contract: every function
executes stencils from first invocation, selection uses semantic/static/IC facts rather
than execution-count hotness, runtime code construction is finite copy/patch/share,
kernels are immutable shared code, and no benchmark identity shapes the result.

## Result

The literature does not support adding more isolated opcode templates as the main strategy.
Deegen and CPython both use copy-and-patch as an emission mechanism, while their useful
speed comes from eliminating work across operations: propagated types, register residence,
ownership/refcount removal, ICs, and larger optimized regions. The current Task 318 neutral
operand-burning result agrees with that distinction.

Four additional bounded experiments survive comparison with the existing ledger.

### 1. AArch64 conditional-compare guard products ([[328]])

LLVM can lower a conjunction of independent comparisons to `cmp; ccmp; ...; b.cond`,
reducing several branch edges to one without hand-emitting machine code. This directly
targets multi-input numeric/shape guards and is especially relevant to [[326]], where a
long region may otherwise pay one unpredictable branch per live-in fact. It complements
[[279-dominance-safe-guard-products]]: Task 279 proves and normalizes the guard product;
Task 328 owns its target-specific rustc/LLVM lowering and disassembly gate.

### 2. Block-local value numbering and effect forwarding ([[329]])

JavaScriptCore's low-latency DFG intentionally uses block-local CSE and register allocation
before paying for a heavier global optimizer. Add the smallest useful reducer over the
existing quoted block: local value numbering for pure numeric/tag/shape operations plus
store-to-load forwarding and dead-store removal for exact local/property/dense locations.
This is a bounded first slice of [[173-effect-token-memory-ssa]], not a competing IR.

### 3. Zero-code frame-state hints and lazy exit reconstruction ([[330]])

JSC models frame updates as `MovHint` stores but emits no machine instruction for them.
Reaching definitions plus liveness reconstruct the exit state only when needed. Adapt that
pattern so register-resident stencils do not eagerly synchronize the canonical guest frame
at every boundary. The hints stay quoted metadata; the only effect is the shared exit
materializer. This refines [[158]] and [[164]].

### 4. Critical-path measurement gate ([[331]])

A 2025 JavaScript AOT study implemented dynamic binary modification for ICs and found that
removing memory accesses did not shorten execution on contemporary hardware. This is useful
negative evidence for this project, which has already produced many instruction-count wins
with flat or negative V8v7 results. Add a measurement gate that classifies candidates by
frontend, branch, memory-latency, and backend pressure and requires a stated critical-path
hypothesis before implementation. This extends [[00]]/[[253]]/[[291]], rather than replacing
the alternating A/B gate.

## Revised experiment order

1. Finish measuring [[326]]; retain it only if static entry guards amortize.
2. Implement [[329]] inside the existing quote so each selected block removes repeated work.
3. Cook [[328]] guard products and inspect emitted AArch64 before broad selection.
4. Continue [[158]]/[[164]] through [[330]] so register residence does not imply eager spills.
5. Keep [[181]]/[[290]] as the main object/call-suite boundary removal.
6. Use [[331]] to reject changes that reduce counts but not the executed dependency chain.

This ordering does not project external speedups onto this VM. Each item remains subject to
the complete supported-suite correctness and alternating A/B rules.

## Primary sources

- Deegen (first submitted 18 November 2024): <https://arxiv.org/abs/2411.11469>.
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>.
- Static BBV, including the two-version result:
  <https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2024.28>.
- Lazy BBV: <https://arxiv.org/abs/1411.0352>.
- JavaScriptCore speculation, block-local optimization, and zero-code `MovHint` state:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- SpiderMonkey CacheIR's `Guard* ; Pure* ; Result` recipe and shared stub code:
  <https://firefox-source-docs.mozilla.org/js/cacheir.html>.
- CPython 3.15 copy-and-patch register allocation, constant propagation, and ownership
  optimization: <https://docs.python.org/3.15/whatsnew/3.15.html>.
- LLVM's AArch64 conditional-compare formation:
  <https://llvm.org/doxygen/AArch64ConditionalCompares_8cpp_source.html>.
- Negative IC/DBM result: <https://arxiv.org/abs/2502.20547>.
- Weval's interpreter partial evaluation result: <https://arxiv.org/abs/2411.10559>.
- Druid's baseline-JIT meta-compilation result: <https://arxiv.org/abs/2502.20543>.

CacheIR, Weval, and Druid reinforce existing Tasks 149/153/171/309; they do not justify
duplicate tasks or third-party runtime dependencies. No score claim changes in this
research task. The accepted checkpoint remains 2083.86 until a complete A/B establishes
a better result.

