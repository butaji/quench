# 258 — Primary-source algorithm research, round eleven

Status: complete

Research additional general VM algorithms under the standing contract: stencil execution
from first entry, no interpreter fallback or hotness gate, rustc/LLVM-cooked finite
templates, runtime copy/patch/share only, and no V8v7 source identity in selection.

## Findings

### 1. The next multiplier is cross-operation information, not more dispatch removal

Deegen's remaining baseline-JIT losses against LuaJIT concentrate in numeric expression
loops and are attributed to the lack of an optimizing tier. Copy-and-Patch separately
reports up to a 10% execution gain from `mem2reg`. Together these are direct evidence
that copy-and-patch is an excellent emission mechanism but not a substitute for keeping
facts and values across operation boundaries.

The existing implementation path is therefore correct but must be reordered around
Tasks 171, 144, 152, 158, 173, 175, and 176: one quoted region fact graph, bounded static
context versions, typed-shape propagation, register residence, MemorySSA, loop
predication, and partial escape materialization. All still lower to the same
`StencilExpr` category and rustc-cooked leaves/supernodes.

### 2. Generate LLVM-visible supernodes; do not hand-enumerate opcode pairs

Copy-and-Patch selects the most specific stencil from AST shape and context and reports
using close to 100,000 general supernodes in its high-level compiler. Those supernodes
let the AOT compiler fold address modes and optimize a larger semantic unit. Vmgen's
warning that static interpreter superinstructions provide only about a 1.1x gain is the
complementary negative result: concatenation that saves only dispatch is not enough.

Task 149's semantic macro should generate bounded supernode derivatives; Task 157's
dynamic program should choose a minimum-cost cover. Rust documents that `#[inline]` is
ignored on externally exported functions, so independent exported handlers cannot gain
cross-op LLVM optimization after copying. Task 251 must measure this ceiling directly.

### 3. Propagate typed shapes, rather than repeating property guards

Typed shapes plus shape propagation eliminated 48% of type tests, reduced code size 17%,
and reduced execution time 25% in Higgs. The useful mechanism is one exact shape guard,
field-representation recovery from the shape, and dominated reuse until an explicit
effect kills the fact. This reinforces Task 152 and the rejection evidence in Task 138:
isolated five-op property fragments repeat exactly the work propagation removes.

### 4. Factor object structure as layout times prototype (Task 259)

Production object models store a compact structure/hidden-class identity on each object;
the shared descriptor carries property layout and prototype-related facts. This project
currently stores both a `ShapeRef` and an `Option<Rc<ObjectCell>>` per object and guards
them separately. Task 259 evaluates a factorized immutable structure kernel
`Structure = LayoutShape × PrototypeIdentity`, so an object stores one structure
identity while layout shapes remain shared across different prototypes.

### 5. ICs need one small effect algebra

SpiderMonkey's CacheIR restricts each recipe to guards, idempotent pure operations, and
one terminal result operation. That is exactly the safe normal form for this VM's
Kleisli/effect category: guard failure can rejoin before effects, pure nodes compose,
and one terminal effect commits. Task 153 already owns it; new handwritten condition
families should stop proliferating after the current measured Task 185 experiment.

### 6. A VM heap must make local `Value` moves non-owning

Deutsch--Bobrow deferred reference counting omits stack/local reference-count updates;
historical measurements summarized in the Berkeley Smalltalk implementation report say
this avoided about 90% of count manipulation. The right adaptation is not another
collector: Tasks 162 and 193 should use precise stencil root maps so register/local moves
are one-word copies and tracing reclaims cycles. Building a zero-count-table subsystem
before the planned VM heap would duplicate reclamation machinery.

### 7. In-object storage remains a first-order memory/locality win

V8 and JavaScriptCore place predicted/common properties inline in the object and spill
only overflow slots to an out-of-line store/butterfly. JSC notes that an out-of-line
property costs an extra load. This confirms Task 156, but it should land on the VM heap
rather than adding more `Rc` layouts.

### 8. Preserve the measurement-first routine

Task 253 should precede another broad runtime rewrite: capture comparable native stacks
for this engine, Node/V8, and Bun/JSC on the same corpus. Then select one owner task from
the largest *differential* stack. Every candidate still requires structural selection
counters, disassembly, semantic tests, full smoke, and alternating A/B.

## Ranked execution order

1. Finish or reject Task 185's already-built whole-condition candidate.
2. Run Tasks 253/254 and Task 251 to measure the differential and composition ceiling.
3. Build Tasks 171 -> 144 -> 152 -> 158 as the cross-operation optimization spine.
4. Apply Tasks 146/177/181 for direct calls and summaries, then Tasks 173/175/176.
5. Replace `Rc` ownership through Tasks 09/148/162/193 and add Task 156 inline slots.
6. Use Tasks 149/157 to grow only measured, macro-generated LLVM-visible supernodes.
7. Evaluate Task 259 after its object-size and guard-count model is recorded.

## Primary sources

- Deegen: <https://arxiv.org/abs/2411.11469>
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>
- Typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>
- SpiderMonkey optimization and CacheIR:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>
- V8 fast properties: <https://v8.dev/blog/fast-properties>
- JavaScriptCore structures and inline/out-of-line slots:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- JavaScriptCore cell/butterfly object model:
  <https://webkit.org/blog/7846/concurrent-javascript-it-can-work/>
- Vmgen superinstructions:
  <https://www.complang.tuwien.ac.at/anton/vmgen/html-docs/Superinstructions.html>
- Rust code-generation attributes:
  <https://doc.rust-lang.org/stable/reference/attributes/codegen.html>
- Deferred reference counting summary and measurements:
  <https://www2.eecs.berkeley.edu/Pubs/TechRpts/1986/CSD-86-287.pdf>

