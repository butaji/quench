# 125 — Borrowed callee dispatch

Status: complete

Every dynamic `Call` currently clones the callee `Value` from its register before
entering `Vm::call_arguments`, then drops that short-lived clone after dispatch. For a
function value this performs one `Rc` increment and decrement even though the caller's
register keeps the function alive for the whole call. Fresh Task 13 native profiles put
`call_arguments<RegisterArguments>` at 127 top-of-stack samples in Richards and 119 in
DeltaBlue; the profiles are `reports/task31-richards-task13.sample.txt` and
`reports/task31-deltablue-task13.sample.txt`.

Change the canonical call boundary to consume `&Value` for the callee while retaining
owned receiver and argument semantics. This is a context-preserving identity morphism:
the borrowed callee is a read-only view of the same tagged word and all user/native,
compile-on-first-call, numeric-image, dynamic-image, and error coproduct arms remain in
one dispatcher. This is distinct from [[104-borrowed-code-image-dispatch]], which kept
a `RefCell` borrow alive across recursive execution and was rejected; linked code images
continue to use their accepted short-lived `Rc` clones.

Acceptance: all release tests and complete V8v7 semantic smoke pass; an exact stable
alternating comparison against [[13-environment-frames]] improves aggregate without
crossing the per-suite floor. Revert and record negative evidence otherwise.

## Result: accepted

The canonical dispatcher now accepts `&Value` for its callee. Dynamic bytecode reads
the tagged word directly through the frame's stable register pointer; top-level and
`Function.prototype.call` entry points borrow their already-owned value. Receiver and
argument ownership, first-call compilation, native dispatch, and linked-image lifetime
rules are unchanged. All 62 release tests and the complete semantic smoke pass.

The focused six-repetition 250 ms comparison in
`reports/task125-borrowed-callee-focused-ab-6/comparison.txt` improved Richards 1.38%,
DeltaBlue 2.27%, and their aggregate 1.82%. The exact full-suite four-repetition 200 ms
comparison in `reports/task125-borrowed-callee-full-ab-4/comparison.txt` raises
aggregate from 1152.54 to **1177.48** (+2.16%). Every component improves: Richards
1.68%, DeltaBlue 3.47%, Crypto 2.42%, RayTrace 1.43%, Earley-Boyer 2.46%, RegExp 2.89%,
Splay 2.29%, and Navier–Stokes 0.70%.

Accepted binary SHA-256:
`9841db36898bc11b1e0cc7df323eb4f2d86ad75c55a2f3a08f004b15dbabd272`.
