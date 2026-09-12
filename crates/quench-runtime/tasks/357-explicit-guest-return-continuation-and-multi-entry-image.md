# 357 — Explicit guest return continuation and multi-entry function image

Status: complete

Introduce the first accepted infrastructure slice of Task 146's direct-continuation call
model without putting another Rust helper on the fast edge. Every compiled function image
now exposes two entries in its immutable `FunctionCallRecipe`:

- `entry` is the existing host entry and executes the host-only prologue;
- `guest_entry` is the address of bytecode label zero and therefore accepts the already
  established pinned guest connector state.

The macro-defined canonical `GuestFrameHeader` gains one named
`RETURN_TARGET_FRAME_WORD_OFFSET` field between the direct-call helper and the existing
effect-resume target. Host-created activations initialize it to the shared immutable exit
kernel. The copied exit-adapter stencil no longer embeds or patches that kernel pointer;
it performs `LoadReturnTarget ; TailBranchTarget`. Future guest calls can therefore place
a caller-specific native continuation in the child frame and transfer to `guest_entry`
without changing the callee image or copying a specialized callee body.

Categorically, this makes the call boundary explicit as data: a function image has a host
entry and a guest entry, while return is a morphism selected by the frame's continuation
object. The immutable function image and shared exit kernel remain reusable `Kernel`
values; only per-activation continuation data varies. In Lisp staging terms, the exit
adapter is one quoted template, instantiated uniformly, and the return address is supplied
by the final call-site environment rather than baked into an ad-hoc code form.

This task intentionally does **not** claim that direct guest calls are complete. The call
site still enters `execute_direct_call`, prepares/reuses a Rust-owned activation, invokes
the host entry, and completes through Rust. The next Task 146 slice must allocate or select
a child guest frame in machine state, initialize its frame/site/argument words, place a
native caller continuation in `return_target`, update the pinned frame and site registers,
and tail-transfer to `guest_entry`. It must preserve explicit miss and exception exits and
must not call a substitute Rust ABI kernel.

## Verification

- `cargo test --release -- --test-threads=1`: **129 passed**.
- Structural tests prove that `guest_entry` is distinct from and follows the host entry,
  the return field is at its generated named ABI offset, the header size is derived from
  `GUEST_FRAME_HEADER_WORDS`, and the adapter bytes are exactly the two named AArch64
  operations above.
- Three alternating 200 ms repetitions against accepted Task 353 binary
  `a02330525ba32d3d371027349f5029c75a5120c9f966644783c66053bafc9688`:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 937 | 922 | -1.60% |
| DeltaBlue | 943 | 948 | +0.53% |
| Crypto | 1801 | 1822 | +1.17% |
| RayTrace | 1979 | 1988 | +0.45% |
| Earley-Boyer | 2782 | 2774 | -0.29% |
| RegExp | 3750 | 3754 | +0.11% |
| Splay | 4140 | 4090 | -1.21% |
| Navier-Stokes | 7022 | 7008 | -0.20% |
| **Geometric aggregate** | **2357.80** | **2354.66** | **-0.13%** |

The change is accepted as neutral infrastructure: all suites remain above the -5% suite
floor and the aggregate remains above the -3% floor. Raw data and binary metadata are in
`reports/task357-explicit-return-continuation-ab-200ms-3/`. Candidate SHA-256:
`1f6a32202a406c156b9bf532a6ad2abb8370d68bd0b02067eb198e41d086bbfc`.

