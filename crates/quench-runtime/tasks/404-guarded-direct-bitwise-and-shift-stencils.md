# 404 — Guarded direct bitwise and shift stencils

Status: complete

Add rustc/LLVM-cooked direct opcode templates for JavaScript `&`, `|`, `^`, `<<`, `>>`,
and `>>>`. Each template is a `Connector -> Connector` leaf with the same next and slow
studs as the existing arithmetic family. It guards both inputs as numbers, guards the
old destination against an `Rc` ownership obligation, applies the canonical ECMAScript
`ToInt32`/`ToUint32` conversion helpers already present in the AOT semantic source, and
writes one canonical numeric raw word.

This is a general bytecode family selected solely by `DynOp::Binary` kind. It contains no
benchmark name, source identity, property spelling, bytecode offset, observed type, or
hotness gate. The six operations are generated with Rust macros so their connector and
rollback behavior cannot drift while the operation itself remains explicit.

The family materially expands total direct cover around calls: in a 100 ms census the
candidate increased Crypto direct-opcode entries from roughly 6.0 million to 16.1
million and reduced semantic-kernel entries from roughly 478 thousand to 229 thousand.
Unit tests compare every operation against the canonical generic numeric semantics,
including signed and unsigned shifts. The release suite, forced-GC suite, and cooker
differential audit pass.

The family is measured together with Task 401 because the latter's total-cover admission
depends on it. The pre-fusion three-pair full screen improved aggregate by 0.76%; the
post-fusion screen improves 1.74%. The shared nine-pair exact comparison at
`reports/task401-call-region-fused/exact-vs-accepted/comparison.md` passes: aggregate
improves **2369.97 -> 2386.83 (+0.71%)**, interval **[+0.26%, +1.23%]**, with every
component above the floor. The accepted binary SHA-256 is
`8fd8b77e7be10c968650c0bacb4cbc65a90497022fbf2a07297b56b877cada00`.

Depends on Tasks 01, 02, 15, 43, 54, 157, 181, 316, 348, 390, and 401.
