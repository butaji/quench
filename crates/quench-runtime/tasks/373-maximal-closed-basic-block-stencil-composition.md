# 373 — Maximal closed basic-block stencil composition

Status: complete

Replace the generic Rust opcode-loop path for every basic block whose complete normalized
bytecode vocabulary is covered by cooked primitive stencils. Selection is structural and
total over the block: no benchmark name, property spelling, source position, runtime
frequency, or hotness threshold participates. Existing coarser superinstructions retain
precedence; otherwise the block is a free-monoid fold of ordinary
`Stencil<Connector, Connector>` leaves.

Every primitive has the same connector contract. Its normal tail is an internal symbolic
`Next` hole resolved when adjacent leaves compose; conditional/taken targets are symbolic
PC labels; return targets the shared immutable exit `Kernel`; and a type, ownership, shape,
or IC miss targets the single shared semantic slow adapter at the current bytecode. Thus a
closed block executes as one native function image with direct internal transfers. The
generic Rust kernel exists only on a real semantic miss, rather than being the default
implementation of the block.

The covered general vocabulary is immediate literal/local load, local declaration/store,
register move, boolean not, guarded numeric add/subtract/multiply/divide and ordered
comparison, monomorphic own static-property get/set, unconditional/conditional control,
and return. The new property leaves read the existing `PropertyIcSite`'s C-layout prefix,
guard object tag plus exact shape, and address the cached fixed slot directly. Traced object
and immediate values stay native; reference-counted strings/functions/regexps rejoin the
canonical ownership path. Compile-time layout assertions bind the Rust cache to the cooked
raw view.

This is the intended hierarchy rather than a thousand-piece-only abstraction:

`primitive stencil -> composed basic-block stencil -> loop/function stencil -> shared exit kernel`.

Every level remains the same categorical morphism and is therefore closed under the same
composition operation. The quoted bytecode is selected before one final emit/link pass,
preserving the Lisp `quote -> rewrite -> eval` boundary.

## Preflight and evidence

Preflight identified `dyn_block_step_impl` as the remaining default for unmatched blocks.
The expected boundary removal was one native-to-Rust call, Rust opcode match loop, and
Rust-to-native return per fully covered block entry. The resulting exact build confirms
that direct coverage grows from 903 blocks / 2,299 opcodes to 1,755 blocks / 5,533 opcodes.
Executable code grows from 345,208 to 590,460 bytes; this is a deliberate first speed/code
size point, with sharing/freeze work left to a later task.

- 138 release tests pass. New tests cover structural all-or-nothing selection, rejection
  of an unsupported computed-property member, native own-property get/set hits, shape-miss
  slow transfer, and the raw/Rust cache layout contract.
- All eight suites pass the complete smoke.
- Short three-pair and five-pair complete-suite comparisons measured +2.26% and +2.19%.
- The required nine-pair upstream-equivalent comparison measured aggregate geometric means
  2284.28 -> 2339.49, **+2.42%**, with a 95% paired-bootstrap confidence interval of
  **[+2.05%, +2.86%]**. Every suite cleared the standing floor; seven improved, RegExp was
  neutral at -0.01%, and Navier-Stokes was neutral at +0.22%.
- One standalone upstream-equivalent candidate process scored **2391.135445752378**. This
  supersedes the prior binary as the current exact artifact but remains far below 10000.
- The differential cooker audit passes at both `-O2` and `-O3` over the expanded 147-symbol
  catalog; all 89 typed holes remain deterministic and confined to declared instruction
  fields.

Reports:

- `reports/task373-maximal-direct-block-smoke.jsonl`
- `reports/task373-maximal-direct-block-ab-200ms-3/`
- `reports/task373-maximal-direct-block-ab-300ms-5/`
- `reports/task373-maximal-direct-block-residual/`
- `reports/task373-current-exact.jsonl`
- `reports/task373-maximal-direct-block-exact-ab-9/`
- `reports/task373-stencil-cooker-audit/`
