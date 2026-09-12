# 142 — Shared immutable RegExp literal kernels

Status: complete

The RegExp suite is the accepted Task 140 binary's lowest component at roughly 225.
A three-second native sample in `reports/task142-regexp-research.sample.txt` shows that
the dominant stack is not regex matching: it is repeated `regex-automata` parsing,
NFA/DFA construction, allocation, and destruction. `DynOp::RegExp` recompiles its
constant pattern every time the bytecode executes, including literals inside loops.

Split the value according to the established memory model. A `RegExpKernel` is the
immutable compiled automaton, cached once in the quoted `DynOp` and shared by `Rc`.
Each literal evaluation still creates a distinct mutable `RegExpValue` instance with
its own `global` and `last_index` state. This preserves JavaScript identity and mutable
state while making the expensive immutable code consume one allocation per literal
site. It is the same categorical arrow at execution: only the representation of its
constant obligation changes from repeated construction to a shared kernel.

Acceptance: two evaluations produce distinct RegExp objects that share one compiled
kernel; all release tests and the full smoke suite pass; the RegExp score and aggregate
improve under alternating A/B without a component crossing the standing -5% floor.

## Result

Accepted. Lowering compiles each RegExp literal once into a
`RegExpLiteralKernel::Compiled(Rc<Regex>)`. If the host matcher rejects the pattern,
lowering stores an immutable error instead and raises it only if that opcode executes,
preserving the prior execution-time failure behavior. Every evaluation allocates a fresh
`RegExpValue` wrapper, so object identity, `global`, and `last_index` remain instance
state while the expensive immutable automaton is shared.

The first implementation stored a write-once `OnceCell` inside `DynOp::RegExp`. It made
RegExp fast, but also enlarged the whole `DynOp` enum and produced cache-sensitive
regressions in unrelated suites in
`reports/task142-regexp-kernel-full-ab-6/comparison.txt`. It was replaced by the compact
precompiled-kernel/error coproduct; no per-execution cell or pattern string remains in
the opcode. This is the stronger kernel/instance split and demonstrates why one large
sum variant can impose cost on every value of the sum.

All 77 release tests pass, including a focused test proving that two evaluations create
different wrapper objects pointing at the same compiled kernel. The complete smoke
suite passes. The clean four-run, 200 ms alternating comparison in
`reports/task142-regexp-kernel-compact-full-ab-4/comparison.txt` improves aggregate
1194.39 to **1616.02 (+35.30%)** and RegExp 234 to **2633 (+1025.21%)**. Every other
suite remains within one percent of baseline, clearing the standing component floor.

Accepted binary: `/tmp/deegen-task142-regexp-kernel-compact`, SHA-256
`bc84ed21e61cc045e68ba116121e4e29bd5732c2f50f4ef17c51fe5459be5803`.
