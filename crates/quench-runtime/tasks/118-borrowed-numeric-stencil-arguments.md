# 118 — Borrowed numeric stencil arguments

Status: complete

Make compiled numeric whole-function stencil images callable from every `CallArguments`
representation, especially the non-contiguous `RegisterArguments` view used by ordinary
JS-to-JS bytecode calls. Current full-suite counters show 2–8 extra compiled images per
suite but zero `inline_entries`: dispatch artificially requires `contiguous()`, so the
numeric image is built and cached but unreachable from normal compiled callers.

Add a borrowed numeric projection to the existing argument interface. Slice arguments
and register-view arguments derive a number directly from their backing slot without
cloning an owned `Value`; the default remains semantically safe for other argument
representations. `LegoJitCode` consumes that interface to initialize its native frame,
then executes the same already-linked function-level stencil. A failed number guard
falls through to the canonical dynamic stencil before any effect occurs.

This is a representation unification, not tiering or benchmark specialization. The
callee and argument list are ordinary bytecode data, image selection is unchanged,
and no source names, source locations, benchmark identity, or runtime heat participate.

Acceptance: unit tests cover contiguous, register-view, numeric, and nonnumeric guards;
all release tests and complete V8v7 smoke pass; structured counters show nonzero numeric
`inline_entries`; an exact alternating six-run A/B against
[[110-constant-condition-control-stencils]] improves aggregate without crossing the
standing -5% per-suite floor. Revert and record negative evidence otherwise.

## Implementation tested

- Added a borrowed `numeric_value(index)` projection to `CallArguments`.
- Slice and fixed-array arguments read their backing `Value` by reference.
- `RegisterArguments` maps the requested argument through its register index and reads
  the corresponding frame slot without cloning or materializing a `Vec<Value>`.
- Generalized `LegoJitCode::call` over `CallArguments` and removed the contiguous-slice
  gate from user-function dispatch.
- Added a structural register-view unit test. Forty-seven release tests passed.

The first complete smoke is
`reports/borrowed-numeric-stencil-smoke.jsonl`; its candidate SHA-256 was
`da0b1a8e7a5bc68581198120af8c130d5ada3535914a705b09de81892c9a35b3`.
All suites remained correct, but `inline_entries` stayed zero. The change therefore
made no benchmark execution enter a numeric whole-function stencil.

## Diagnostic result

Temporary counters separated numeric images, call candidates, argument rejections,
and successful entries. The full diagnostic smoke is
`reports/borrowed-numeric-stencil-counter-smoke.jsonl`; the diagnostic binary SHA-256
was `028a66c6222b865aa7e65ef6e2693ab74b636df5d550eed5ef3d9bba20022fcd`.

Only Crypto built numeric images (3) and Earley-Boyer built numeric images (6). Every
suite recorded zero numeric-call candidates, zero argument rejections, and zero inline
entries. An environment-gated trace identified Crypto's images as `cRevert`, `nNop`,
and `barrettRevert`. The focused `first_loop_call_uses_native_entry` test, strengthened
temporarily with exact counters, did compile and execute one numeric image. This proves
the numeric entry mechanism works in isolation while the V8v7-produced images are not
on an exercised call path. The earlier inference that `compiled_images -
compile_attempts` measured numeric images was false: top-level script images also
contribute to `compiled_images`.

## Decision and learned constraint

Rejected and fully reverted without a long A/B because the required activation signal
was absent. The rebuilt executable is byte-identical to the accepted Task 110 image:
`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`.
The baseline's 46 release tests pass.

Do not optimize an interface solely because dormant compiled artifacts exist. The
routine must measure three distinct facts: an image was built, dispatch considered the
image, and execution entered it. Future coarse-region work must start from runtime
entries in the canonical dynamic stencil path, not from compile-count differences.
