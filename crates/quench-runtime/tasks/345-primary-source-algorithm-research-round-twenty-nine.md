# 345 — Primary-source algorithm research, round twenty-nine

Status: complete

Reconcile the next performance ideas against the code that actually executes and the
existing task ledger. The goal is not to mint another parallel roadmap. It is to turn
the research into one ordered set of already-canonical work items, with V8v7 held out as
the acceptance benchmark and no source-name, benchmark-name, or runtime-hotness-driven
selection.

## Current diagnosis

Stencil composition is working, but composition alone cannot remove work frozen into
the leaves. Concatenating fragments that still load operands from `InlineSite`, call a
Rust semantic helper, box intermediate values, or construct a Rust-owned guest frame
only removes the seam between those fragments. The next gains must change the selected
native program at a coarser abstraction level.

The current runtime is also materially ahead of the stale representation critique: it
already has an eight-byte NaN-boxed `Value`, immutable hash-consed shapes, fixed property
slots, shape/prototype ICs, dense-array storage, and a nonmoving object heap. Replacing
those with similarly named systems would duplicate facts. The remaining gaps are typed
array representations, direct guest calls, broader operand burning, cross-operation
register residence, and complete costed coverage.

## Canonical implementation order

1. **Cross-function boundary erasure — [[20]] first, [[146]]/[[181]] for the remainder.**
   Implement the already-measured exact-target straight-line leaf slice as a pure quoted
   graph rewrite. Then widen SCC-bottom-up to control flow and nested calls. Calls that
   cannot be inlined use the fixed guest-stack call-main and return-continuation
   stencils. Do not add another Rust-helper-shaped call fast path: [[337]] already
   falsified that intermediate design.
2. **Typed operand burning — [[316]].** Extend the cooked relocation vocabulary from
   successors and the first register stride to register/local byte offsets, literals,
   IC data, direct targets, and continuations. A successful native leaf must not decode
   its immutable operands from a site record.
3. **Total costed block cover — [[157]] plus generated matcher [[272]].** Replace exact
   whole-slice recognition with shortest-path selection over
   `(micro_op_position, connector_context)`. Primitive stencil or kernel leaves ensure
   total coverage; AOT-cooked supernodes and block/loop candidates compete using one
   cost record. The same selected-plan value estimates and materializes, preserving the
   quote/rewrite/eval boundary.
4. **Typed region versions and register residence — [[144]], [[158]], [[326]].** Guard
   once at a block/loop entry, keep `I32`/`F64` intermediates in physical registers,
   and box only at a side exit. Cap versions with named constants; use static structure,
   never a hotness counter. The mixed [[326]] result means guard products and effect
   forwarding must improve before broad default enablement.
5. **Inline IC slabs — [[145]], after the call and operand substrates.** Install short
   shape/offset and exact-call guards in the site slab, with shared immutable miss and
   megamorphic kernels. Existing shapes and fixed slots are reused; this task is native
   placement, patching, and continuation work, not a new object model.
6. **Code locality and CFG layout — [[189]], [[276]], [[322]].** Measure generated-code
   to helper/kernel distances, build one near-code immutable kernel island, separate
   cold obligations, choose static fallthroughs, and apply strictly bounded tail
   duplication. Named architecture constants own all ranges and budgets.
7. **Packed numeric array representations — [[265]].** Extend the current dense backing
   into a monotone representation lattice with raw numeric storage and explicit
   generalization side exits. This is required for C-like Navier-Stokes/Crypto inner
   loops; another generic tagged-array stencil is not.
8. **Heap, strings, and RegExp after the boundary work — [[148]], [[162]], [[190]],
   [[87]].** These are real general-engine work. They remain separately tracked rather
   than being misreported as the current reason a composed arithmetic stencil is slow.
9. **Finish the missing linker/cooker/entry infrastructure — [[346]], [[347]],
   [[348]].** Export loop-header native entries without adding hotness detection, test
   the complete rustc/LLVM configuration space fail-closed, and make final linking an
   explicit size/layout pass followed by one allocation/copy/patch pass.

## Required algorithms and measurements

- Every candidate level—operation, superinstruction, block, loop, and function—implements
  the same typed connector interface. `Kernel` is a shared immutable morphism;
  `StencilTemplate -> StencilInstance` binds per-use holes; a closed identical instance
  may be interned by its complete structural/binding key.
- The selector's cost record includes native bytes, helper edges, dynamic site loads,
  taken seams, register pressure, and copied-instance memory. Constants are named and
  architecture-owned rather than embedded numbers.
- Every accepted score experiment reports all eight V8v7 components, aggregate score,
  linked code bytes, helper calls or native samples for the targeted boundary, and the
  candidate/baseline binary hashes. Rejected candidates remain documented and reverted.
- V8v7 is an evaluation corpus only. Template families may be derived from opcode/effect
  grammars or an explicitly separate training corpus, never from V8v7 function names,
  source strings, or constants.

## Evidence and source synthesis

- Deegen burns runtime constants into continuation-style stencils, separates hot/cold
  code, gives calls distinct return continuations, and places IC code in inline slabs:
  <https://arxiv.org/abs/2411.11469>. The arXiv submission date is 18 November 2024,
  not 2026.
- Copy-and-Patch uses pattern covering, runtime-constant holes, GHC-style connector
  registers, Sethi–Ullman planning, and AOT-cooked supernodes:
  <https://arxiv.org/abs/2011.13127>.
- LLVM's instruction selectors support declarative costed covering, and its block
  placement/tail-duplication machinery makes code-size versus fallthrough an explicit
  tradeoff: <https://llvm.org/docs/CodeGenerator.html>,
  <https://llvm.org/docs/GlobalISel/InstructionSelect.html>, and
  <https://www.llvm.org/docs/doxygen/MachineBlockPlacement_8cpp_source.html>.
- Static basic-block versioning shows that useful type-context versions can be selected
  without runtime hotness detection: <https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>.
- V8's shared builtins and short-builtin-call work support immutable shared kernels plus
  near-code placement, especially on Apple silicon:
  <https://v8.dev/blog/embedded-builtins> and
  <https://v8.dev/blog/short-builtin-calls>.
- V8's adaptor-frame removal supports one fixed guest-frame layout with actual argument
  count rather than per-call argument adaptation: <https://v8.dev/blog/adaptor-frame>.
- CPython's copy-and-patch JIT keeps LLVM at build time and emits optimized semantic
  micro-operations into W^X code pages: <https://peps.python.org/pep-0744/>.

## Ledger result

Most actionable ideas map to existing canonical detail files above. The audit found three
genuine omissions and records them as [[346]]–[[348]] rather than silently stretching
nearby tasks: the loop-header native-entry table, the complete cooker configuration
matrix, and the two-pass final linker. This item records the research and crosswalk;
[[301]] owns the living priority order, [[00]] owns the per-experiment process, and each
implementation item owns its code, evidence, and acceptance result.
