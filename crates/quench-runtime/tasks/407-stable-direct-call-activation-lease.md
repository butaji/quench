# 407 — Stable direct-call activation lease and connector preflight

Status: complete

Outcome: rejected; runtime changes reverted.

This task tested whether the direct-call arm should borrow the already-published reusable
`DynFrame` in place instead of moving its `Box` out of and back into the call site's
`RefCell` for every invocation. The proposed representation had one canonical availability
fact: a non-null `InlineCallTarget::reusable_activation`. Claiming replaced that pointer with
null while leaving the owning `Box` in stable backing storage; releasing republished the same
pointer. A separately owned activation handled overlap when the cached frame was unavailable.

This is the Lisp/data-first shape of the lease: availability is derived from one published
value rather than duplicated between a raw pointer and `Option<Box<_>>`. It is also compatible
with the call category: both cached and owned physical realizations implement the same
`CallContext -> ReturnContext` transition and rejoin one continuation.

## Correctness finding

The first implementation exposed a real reentrancy hazard. While the direct path held the raw
lease, a recursive slow call could still take the backing `Box`, moving or freeing the frame
behind the active pointer. The corrected prototype made the published pointer the canonical
availability fact: a null pointer forced the slow path to allocate without touching backing
storage. A focused regression test proved that the claimed frame did not move, and Splay no
longer crashed. The complete release and forced-GC suites both passed with 158 tests.

## Measurement

The repaired three-pair, 200 ms screen at
`reports/task407-stable-activation-lease-quick-fixed/comparison.txt` measured **2506.42 ->
2529.99 (+0.94%)** with no component-floor violation. That was enough to run the prescribed
nine-pair exact gate, not enough to accept the change.

The exact randomized comparison against the accepted Task 368 binary is recorded at
`reports/task407-stable-activation-lease-exact/comparison.md`:

| Metric | Baseline | Candidate | Change | 95% paired bootstrap CI |
|---|---:|---:|---:|---:|
| aggregate | 2405.61 | 2407.94 | +0.10% | [-0.73%, +0.95%] |

The confidence interval includes no change. The tested candidate SHA-256 is
`42d897396510ffa2b9e3cf539ce48dbd3695b9f6f07cef6cf210cd0d5647f656`. The ownership
shuffle is therefore not a demonstrated bottleneck, and the runtime change and its
prototype-only test were removed. The reentrancy rule remains a required invariant for any
future native activation lease.

## Rejected region micro-experiments

Two small Task 385 preflights were also rejected and fully removed:

- Enabling existing block-local forwarding inside loop regions found only three to nine
  static forwarded loads and caused a **43.77% Navier-Stokes regression** in the bundled
  screen. Evidence: `reports/task407-loop-local-forwarding-trace.log`.
- Adding six burned-operand bitwise/shift leaves measured **-1.71% aggregate**, with no
  Crypto improvement and a **-3.03% Navier-Stokes regression**. Evidence:
  `reports/task407-burned-bitwise-only-quick/comparison.txt` and
  `reports/task407-burned-bitwise-trace.log`.

These results reinforce the existing Task 385 requirement: arithmetic work must remain in a
register-resident multi-operation context. More frame-based leaves and local rewrites do not
remove boxing, conversion, frame traffic, or helper seams and therefore cannot produce the
needed scale of improvement.

