# 149 — Build-time semantic derivatives and type-lattice stencils

Status: planned

Define each bytecode's semantics once in a Rust macro input that makes type checks,
conversions, effects, exceptional exits, and result representations explicit. At build
time, interpret that quoted definition over a finite type lattice to derive:

- the generic semantic kernel;
- guarded `I32`, `F64`, boolean, shape, call-target, and element-kind templates;
- strength-reduced variants when an input fact makes a check or conversion redundant;
- slow-path outlines and the patch obligations needed to rejoin the common ABI.

rustc/LLVM compiles these finite derivatives AOT. Runtime work is only selection,
copying, patching, and composition; it does not invoke LLVM and does not implement a
second bytecode semantics. This extends Tasks 02, 10, 37, and 52 with the controlled
specialization algorithm rather than adding hand-written variants indefinitely.

In Lisp staging terms the macro input stays quoted, lattice propagation and rewrites run
to a fixed point, and the finite catalog is evaluated once by rustc. In categorical
terms each derived arrow retains the same semantic source and explicit input/output
context, while coproduct slow exits preserve the generic meaning.

The algorithm is based on Deegen's controlled de-abstraction and specialized-bytecode
generation (<https://arxiv.org/abs/2411.11469>) and the instruction-substitution staging
model in speculative staging (<https://arxiv.org/abs/1310.2300>).

Acceptance: one macro definition generates generic and at least two specialized forms;
differential/property tests show every derivative agrees with the generic semantics on
its accepted domain and exits correctly otherwise; catalog size and build time are
bounded by named constants; complete V8v7 A/B improves.

Research round six makes this a prerequisite of completing the general direct-stencil
path, not a later refinement. Deegen's baseline-JIT ablation reports a 2.69x average
slowdown (maximum 5.72x) without specialized bytecode variants, compared with 1.45x
average without inline caches and 1.21x without type-based optimization. These figures
must not be multiplied or treated as JavaScript predictions, but they establish the
priority of a controlled Cartesian product over operand location, operand value,
input/output representation, pass-through register context, and common semantic form.
The generator must deduplicate equivalent derivatives and enforce named catalog-size
and code-size budgets.

## Round-twelve refinement

Treat finite derivative generation as an offline form of context specialization. A
worklist keyed by `(semantic_block, abstract_context)` clones only reachable semantic
paths, folds constant bytecode operands and tag predicates, and stops at explicitly
dynamic effects. The output is still a finite rustc source catalog cooked once at build
time; guest bytecode never invokes LLVM. This borrows weval's context-specialized block
algorithm without adding its Wasm implementation or a runtime partial evaluator:
<https://cfallin.org/pubs/pldi2025_weval.pdf>.

## Round-forty-six binding-time template refinement

Make the static/dynamic boundary an explicit generated fact for every semantic operand.
Static-at-link inputs include the opcode, literal, local/property operand, control target,
and any proved representation, shape, callee, or effect fact. Dynamic inputs are the values
and heap state only known when native code runs. A binding-time analysis over the one quoted
semantic definition generates reusable graph templates parameterized by the static inputs;
normalization and rustc/LLVM cooking then derive the finite machine-code catalog.

This is the stencil-compatible form of the 2026 Partial-Evaluation Templates algorithm.
That work reports up to 36% less partial-evaluation time and up to 17% less warmup without
peak-performance loss. We borrow the AOT binding-time/template construction, not GraalVM,
runtime LLVM, or a second interpreter:
<https://doi.org/10.1109/CGO68049.2026.11395215>.
