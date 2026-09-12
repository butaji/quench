# 136 — Coarse static-property block morphisms

Status: complete

Task 132 measured 4,319,715 residual static-property block entries after all accepted
selectors. Task 113 proved that another Rust helper which loops over and decodes the
original `DynInstr` stream is slower. This task therefore erases dead bytecode
temporaries and lowers a whole structural block grammar to one rustc/LLVM stencil plus
one shared immutable fast connector.

Initial general grammars, selected only by bytecode structure and liveness, are:

- `LoadLocal(receiver); GetStatic; StoreLocal(result); Jump`; and
- two repetitions of `LoadLocal(receiver); LoadLocal(value); SetStatic`, followed by
  `Return(undefined)`.

Property names, local slots, registers, shapes, and branch targets remain data in
`InlineSite`/IC records; no source path, benchmark name, property spelling, or runtime
hotness participates in selection. Dead-temporary proofs license the block morphism to
operate directly on locals instead of materializing the bytecode registers. The shared
connector must call the canonical `get_static_cached`/`set_static_cached` semantics. An
IC miss or semantic edge must tail-rejoin the original block at its first bytecode
without partially applying a store sequence.

Categorically, each grammar is a block-level `Connector -> Connector` morphism, not a
flat chain of tiny property stencils. Its immutable rustc-generated template and shared
kernel are compatible with the same stencil category and compose with function/loop
nodes through the ordinary connector contract.

Acceptance: selector rejection tests for register/liveness mismatches; semantic native
entry tests for own-property hits, IC warmup/miss, nullish errors, aliasing, and heap
values; release test suite; selector/residual counters proving the path is wired; full
eight-suite smoke; alternating full-suite A/B. Reject and revert any variant that does
not improve the stable aggregate.

## Result: rejected and reverted

The first grammar was implemented as one rustc/LLVM stencil. Its shared connector read
the existing `PropertyIcSite`, invoked the canonical `get_static_cached`, wrote directly
from the receiver local to the result local, and jumped to the bytecode target. Exact
flow and global dead-temporary selector tests plus all 76 release tests passed. The full
eight-suite smoke passed. In a short EarleyBoyer residual run the exact
`LoadLocal,GetStatic,StoreLocal,Jump` shape fell to 166,025 entries, proving real wiring.

Nevertheless, `reports/task136-property-get-full-ab-4/comparison.txt` measured the exact
Task 133 baseline at 1185.84 and the candidate at 1176.75: **-0.77%** aggregate.
Richards, DeltaBlue, RayTrace, EarleyBoyer, and Splay each regressed roughly 1%. The
indirect Rust helper boundary costs more than eliminating four generic bytecode
dispatches. The implementation and tests were reverted; the source returns to the
Task 133 implementation.

Learned constraint: a winning property stencil must consume a stable shape/slot view
directly in rustc-generated machine code. Another helper—whether per opcode or per
block—is rejected. Task 137 owns the prerequisite object ABI/IC descriptor work.
