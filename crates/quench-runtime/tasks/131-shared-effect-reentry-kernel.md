# 131 — Shared one-step effect reentry kernel

Status: complete

Add one immutable shared effect kernel compatible with the stencil connector ABI. An
opaque effect leaf tail-transfers to a per-function adapter that invokes the singular
Rust semantic definition for exactly one bytecode, then uses the existing target table
to reenter the next labeled stencil. Calls, construction, allocation, generic property
misses, and exception-capable operations can thereby appear inside a coarse composed
region without making the remainder of its semantic block execute generically.

This is the explicit effect boundary: pure/validated leaves remain direct machine code;
the kernel owns Rust mutation and error conversion; success is a morphism back to an
unknown context that must be revalidated before typed numeric code resumes. One kernel
mapping is shared globally and does not increase instance memory; each function carries
only symbolic branches/adapter data required for reentry.

Acceptance: one-step execution and exception tests, reentry after calls and allocations,
no duplicate semantics, labels for every legal continuation, executable-memory accounting,
and counters proving fewer whole-block generic entries. Benchmark independently before
combining with Task 130 so an effect connector cannot conceal a regression.

## Evidence and work log

- The existing `dyn_single_step` adapter already delegates exactly one instruction to the
  singular `execute` semantic definition, converts thrown and ordinary errors through
  `exceptional_pc`, updates `current_site`, and returns the existing target-table address.
  Task 131 therefore wires this adapter; it does not add another bytecode evaluator.
- The Task 128 full-suite frequency artifact contains 554,411 one-op block-kernel entries.
  Existing terminal/jump stencils already cover almost all of them; the remaining general
  one-step grammar includes 28,002 `JumpIfFalse` entries and 27 `ForInNext` entries. This is
  the first deliberately narrow runtime wiring: it reduces copied code and proves reentry
  without recreating Task 117's long chains of tiny opcode stencils.
- Blocks with one opaque operation and at least six total operations account for only
  210,440 entries. They remain out of this task until a coarse AOT prefix/suffix stencil can
  retain values across the effect boundary; composing their individual opcode leaves would
  knowingly repeat Task 117's measured regression.

## Result

The runtime now selects every otherwise-generic one-bytecode block as a quoted symbolic
dispatch leaf. All such leaves in a function target one 16-byte patched adapter; that
adapter tail-branches to one shared immutable 32-byte `Kernel<Connector, Connector>`.
The kernel is produced by the `define_helper_kernel!` Rust macro, calls `dyn_single_step`,
and reenters the function through the existing site/target table. The singular `execute`
definition still owns semantics. Name snapshots refresh across the effect exactly as they
do in the existing block kernel.

Seventy-four release tests pass. New executable tests cover allocation followed by direct
return, global lookup and native call followed by direct return, uncaught throw exit,
structural selection, connector-category compatibility, and shared kernel/template
identity. The complete eight-suite smoke passes in
`reports/task131-effect-reentry-final-smoke.jsonl`.

`reports/task131-effect-reentry-stats.txt` shows the kernel is linked and entered in every
suite. It reduces copied function-image memory from 360,040 to 357,300 bytes over the same
smoke set; the one shared 32-byte kernel is outside that per-function total. The four-run,
500 ms alternating full-suite result in
`reports/task131-effect-reentry-full-ab-4/comparison.txt` is score-neutral at 1176.31 to
1176.22 (-0.01%), with every suite above the standing -5% floor. This task is retained as
memory and composition infrastructure, not claimed as a speedup. Final executable:
`/tmp/deegen-task131-effect-reentry`, SHA-256
`69bdc0e22d2a7bdb81ca8df5b393eebff952064455683d730f915396030f49a7`.
