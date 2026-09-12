# 167 — Own-property method-call condition stencil

Status: complete

Add a general rustc/LLVM-cooked coarse stencil for the normalized bytecode block:

`LoadLocal(root); GetStatic(receiver); GetStatic(method); Call(); JumpIfFalse`

Both property reads use the existing monomorphic own-property descriptors. On a double
hit, the stencil passes borrowed raw values directly to the shared call kernel and
branches on the returned truthiness without materializing the root, receiver, callee, or
result virtual registers. Any property miss rejoins the canonical whole-block slow
stencil before the effect occurs.

The initial vocabulary deliberately accepts only zero-argument calls. Argument registers
would otherwise need their own explicit materialization morphism because this stencil
erases intermediate virtual-register writes. Calls with arguments remain on the generic
stencil block.

Add a first-class effect-resume connector to the uniform stencil ABI. A throwing call is
performed exactly once; the Rust call kernel selects the catch/exit target and the AOT
stencil tail-branches through the resume connector to that already-linked stencil. This
is a small concrete precursor to Task 164's general canonical side-exit frame state, not
its completion.

The selector is purely structural and liveness checked. It does not inspect benchmark
identity, source text, runtime hotness, or execution counts. The new stencil is another
`Connector -> Connector` morphism and composes at function level through the existing
quoted free-monoid expression.

Acceptance: selection and rejected-near-miss tests; true/false result tests; a throwing
method test proving the effect is not replayed; complete release tests and V8v7 smoke;
an alternating Richards A/B against the exact accepted Task 160 binary followed by a
full-suite A/B if targeted evidence is positive. Accept only if the full aggregate
improves and every component clears the standing regression floor.

## Result

Implemented and measured, then rejected and reverted. The candidate generated one
rustc/LLVM AOT stencil with two direct own-property guards, a borrowed call-kernel
boundary, direct truthiness branching, and a separately composable effect-resume
connector. The selector accepted exactly one additional Richards block containing five
opcodes: compiled direct coverage rose from 38 blocks/89 opcodes to 39 blocks/94 opcodes.
This proves the candidate was wired and executed through the intended stencil image.

The first implementation appended the 16-byte resume adapter to every direct function,
growing Richards code from 13,624 to 14,852 bytes. It measured -1.59% on a ten-pair
Richards A/B. A derived `needs_resume` property then confined the adapter to only the
function containing this effectful stencil, eliminating 1,040 bytes of unnecessary
copies. The second ten-pair A/B still measured 664 to 659, or **-0.75%**. The general
stencil was therefore not accepted and no full-suite comparison was justified.

All 82 candidate release tests passed, including structural/liveness rejection,
true/false results, and a throwing call-count test proving the resume path did not replay
the effect. Complete candidate smoke also passed. Evidence remains in:

- `reports/task167-method-call-condition-smoke.jsonl`
- `reports/task167-method-call-condition-richards-ab-10/comparison.txt`
- `reports/task167-method-call-condition-conditional-richards-ab-10/comparison.txt`

The result is architectural evidence for Tasks 146 and 163: erasing property
materialization while retaining a Rust VM call boundary is insufficient. The next call
optimization must compose caller and callee continuations or customize the callee image,
not merely wrap the same heavyweight call boundary in a larger stencil.
