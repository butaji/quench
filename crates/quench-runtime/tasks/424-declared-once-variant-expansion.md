# 424 — Declared-once operator variant expansion with proven-type elision

Status: planned

Deegen's bytecode DSL lets an operator's semantics be written exactly once, in the fully
generic form, with the type-specialized forms (e.g. `AddVV`, `AddVN`, `AddNV` for
variable-variable, variable-constant, constant-variable addition) declared as `Variant`s of
that one definition rather than written out by hand. Because each variant declares which
operands are proven to be a given type (e.g. a constant proven `tDouble`), Deegen's own
optimizer sees that a runtime type check like `rhs.Is<tDouble>()` must be `true` for that
variant and deletes the check entirely — one semantic source function turns into N
statically-optimal specialized opcodes with zero duplicated logic and zero residual checks.

Quench has many individually-authored specialized-arithmetic and guarded-operation tasks
(10, 96, 100, 118, 120-124, 375, 404 and others), each hand-writing one specialization. That
is the *outcome* Deegen's variant system produces automatically, but there is no task for the
*generative framework itself*: declare one canonical semantic function plus a set of proven-
operand-type facts per variant, and have the stencil cooker both instantiate the specialized
code and statically erase the now-provably-true guards, instead of a person writing and
proving each specialized stencil by hand. This is a meta-task: it does not add a new
optimization, it proposes replacing repeated by-hand specialization work with one declarative
mechanism that the register-region/guard-elision machinery already has the pieces for
(Tasks 25, 60, 223, 231-234 cover guard-elision proofs; this asks that those proofs be driven
from variant declarations rather than one-off derivations per task).

Acceptance: at least two existing hand-specialized stencils (e.g. from Tasks 96 and 118)
re-expressed as variants of one shared semantic definition with equivalent generated code
(same disassembly modulo cosmetic differences) and no hand-elided guard left unexplained by
a variant's declared type facts; full correctness suite.

Primary source: sillycross, "Building the fastest Lua interpreter automatically"
<https://sillycross.github.io/2022/11/22/2022-11-22/> (Bytecode Variant / type-specialized
codegen section).
