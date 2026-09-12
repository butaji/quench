# 251 — Validate whether the stencil-composition boundary imposes a register-allocation ceiling

Status: planned

**Architectural-flaw hypothesis, stated plainly:** [[30-stencil-register-allocation-quality]]'s
own text explicitly scopes itself away from this question — "This is orthogonal to the
categorical composition structure — it is an implementation-quality concern of a single
stencil's own codegen, not a composition-level optimization." That is a reasonable
scoping decision for what [[30]] itself should cover, but it means **no task in this
project's 250-item ledger currently asks whether composing many independently-AOT-cooked
stencils together, each with its own fixed entry/exit ABI, imposes a structural ceiling
on register allocation quality that no amount of per-stencil codegen improvement or
additional direct-stencil coverage can close** — regardless of how many more opcode
families reach [[36]]'s direct-stencil status, a value that must round-trip through a
fixed frame slot at every stencil boundary (because each stencil was compiled
independently, not as part of one whole-function register allocation the way a single
`rustc`/LLVM compilation unit would) can never stay in a register across that boundary
the way it would inside one normally-compiled function. This is the single most
consequential unexamined question for the [[15]]'s 10000-score gate: if this ceiling is
real and significant, the current strategy — "find more patterns, give them direct
stencils, repeat" — has fundamentally diminishing returns *below* the target regardless
of how completely it is executed, and the actual path to 10000 requires either whole-
region register allocation across composed stencils (a genuinely different, harder
problem than anything [[128]]/[[157]]/[[240]] currently commit to solving at the
register level, only at the guard/dispatch level) or accepting a lower structural
ceiling than a true single-pass-compiled optimizing JIT achieves.

**This is testable now, directly, without waiting for more stencil coverage.** Pick one
region already maximally composed under this project's most advanced composition
machinery ([[117]]/[[128]]/[[131]], or whichever is furthest along at the time this task
starts — richards' scheduler inner loop is the natural candidate, being this project's
most measured and iterated-on hot path) and compare it, instruction-for-instruction and
register-spill-for-register-spill, against a hand-written Rust function performing the
exact same semantic operation compiled as one ordinary `rustc`/LLVM unit (not through
the stencil pipeline at all — a direct, non-stencil baseline). A persistent, structural
gap (not closed by [[30]]'s per-stencil codegen quality work) in register-spill traffic
or redundant memory round-trips at composition boundaries specifically (not elsewhere)
is direct evidence the hypothesis is correct; a gap that closes once [[30]] and existing
composition tasks are applied is evidence the ceiling hypothesis is *false* and the
current strategy is sound as-is.

Concrete steps:
1. Select one maximally-composed hot region and its semantic equivalent, written as one
   ordinary (non-stencil) Rust function.
2. Compile both, disassemble both, and directly count: total instructions, register
   spills/reloads, and specifically how many values cross a stencil composition boundary
   via memory (a frame slot) versus how many would, in the hand-written version, simply
   stay in a register for the whole function.
3. Isolate whether any measured gap is attributable specifically to the composition
   boundary (supporting the ceiling hypothesis) or to something else entirely —
   incomplete guard coverage, a missing direct stencil, suboptimal codegen within one
   stencil (which is [[30]]'s scope, not this task's) — before drawing a conclusion.
4. If the ceiling hypothesis is confirmed, state what would actually be required to
   close it (a genuinely different mechanism: treating a whole composed region as one
   register-allocation unit at cook time, which is a materially harder AOT-pipeline
   problem than anything currently scoped) as a follow-up task, rather than leaving the
   finding unactionable.

Acceptance: a direct, measured comparison exists between one maximally-composed stencil
region and its hand-written non-stencil equivalent, with the gap (if any) specifically
attributed to the composition boundary or ruled out as such; the result is stated as a
confirmed or refuted hypothesis, not left ambiguous; if confirmed, a concrete follow-up
task is opened with the specific mechanism needed to close the gap; [[15]]'s framing of
"more direct stencils closes the gap to 10000" is explicitly revisited in light of the
result, since a confirmed ceiling would mean that framing is incomplete regardless of
how much further stencil-family work proceeds.

No external primary source needed — this is a direct empirical measurement against this
project's own compiled output, following the same disassembly-verification discipline
already established by [[78]], [[102]], [[209]], and [[238]].

Two primary-source facts now sharpen the hypothesis. Copy-and-Patch reports that its
prototype `mem2reg` pass improved execution by up to 10% (at roughly three times compile
cost), proving that memory-resident locals remain material even after copy-and-patch.
Rust's Reference also states that `#[inline]` is ignored on externally exported
functions. Since each cooked handler is an exported symbol, LLVM cannot be expected to
recover cross-handler optimization merely from an inline annotation. The comparison
must therefore include a macro-generated whole-region exported handler whose internal
semantic helpers are inlinable; if that closes the gap, Task 149/157 generated
supernodes are sufficient, while a remaining gap motivates Task 158's explicit context
planner.

Sources: <https://arxiv.org/abs/2011.13127> and
<https://doc.rust-lang.org/stable/reference/attributes/codegen.html>.
