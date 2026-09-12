# 78 — Verify explicit tail calls across all stencil dispatch points

Status: complete

`stencil-aot/handlers.rs` enables `#![feature(explicit_tail_calls)]`, but not every
inter-stencil control transfer necessarily uses the `become` keyword. A dispatch-ending
handler that falls back to an ordinary call breaks the guaranteed-tailcall property that
copy-and-patch stencils depend on: no stack growth across opcode boundaries, and
call-argument register threading preserved end to end.

Audit every `pub unsafe extern "C" fn deegen_dyn_*` handler in `stencil-aot/handlers.rs`
that ends by transferring to a successor stencil (jump, jump_if_false, loop backedge,
call dispatch, macro-generated arithmetic/comparison families at lines 325 and 426) and
confirm the transfer is a `become` expression, not a plain call. Any handler missing
`become` at its exit point is a correctness-adjacent performance bug: it will still run,
but will not compose into the flat tail-call chain the rest of the design assumes.

Acceptance: a documented pass over every dispatch-ending handler, `become` present at
every exit; a regression check (e.g. disassembling a linked function image, or a
compile-time lint) that fails if a new handler is added without `become` at its final
control transfer; no behavior change expected since this is an audit, not a rewrite.

Result: every dynamic continuation in `stencil-aot/handlers.rs` is either a direct
`become` or expands from `continue_or_slow!`, whose two exits are both `become`.
`build.rs::extract_stencil` discovers every next, slow, and taken-branch relocation and
calls `validate_tail_branch` for each one. The build fails unless the relocated AArch64
instruction has the unconditional tail-branch opcode (`B`), rejecting `BL` calls and
non-branch exits. This checks the extracted machine code rather than trusting source
spelling, so macro-generated handler families are covered automatically.

Related: 36 (direct opcode stencils), 30 (stencil register allocation quality).
