# 384 — Typed property-flow numeric-region admission experiment

Status: complete

Test whether downstream property-value type is the missing context for Crypto's existing
numeric stencil regions. This is a general quote-stage experiment: derive whether each
static property result is consumed as a number or merely copied, then either admit a
typed property view or reject the mismatched region. No property spelling, benchmark
identity, execution counter, or hotness threshold participates.

## Preflight

A 20 ms Crypto trace of the accepted Task 381 binary recorded 160,922 region-guard
failures and 220,028 successes. Of those failures, roughly 138,000 were the same
structural error: a block began by reading an object-valued `.array` property, while the
numeric-region guard required every static property to be a number. Existing bitwise
stencils were already present inside the region algebra, so this appeared to prevent a
property-load/bitwise continuum rather than indicate a missing opcode.

## First realization: copy-safe property views

The first candidate added `CopySafe | Number` property kinds derived through register and
local-copy flow. Copy-safe values included immediates and traced object pointers but
excluded Rc-owned strings/functions/regexps. Focused tests passed and Crypto validation
remained correct, but the candidate slowed Crypto from roughly 1,750 to 650 in alternating
20 ms screens. It made short prologue blocks pass the guard and execute as many tiny
stencil leaves; the previous fast failure entered one coarse shared kernel. It also
accidentally rejected one dense loop through an over-conservative cross-block origin,
which was fixed before the second realization. Rejected binary:
`/tmp/deegen-task382-copy-safe-property-candidate-v2`, SHA-256
`f78a75235bb411fbb49042006eca1d6dbbd7e3b4fff3e6fe87b2e9a693b7a529`.

## Second realization: reject copy-only property regions

The second candidate kept numeric property semantics unchanged and rejected a numeric
region statically when a property result only flowed through copies. Crypto guard failures
fell from 160,922 to 698 while successful long-loop entries and 1,878,456 counted loop
iterations were preserved. Nevertheless the three-pair 100 ms full-suite screen in
`reports/task382-reject-copy-property-ab-3/comparison.txt` failed: aggregate 2279.35 to
2260.43 (-0.83%), Richards -6.86%, RayTrace -3.06%, and Earley-Boyer -3.18%. Rejected
binary: `/tmp/deegen-task382-reject-copy-property-candidate`, SHA-256
`a69bd1dd024709332787ef632cdfd7b3d9d74f7b0de5518fcb3e989173a3183d`.

Both source realizations were removed. The rebuilt binary is byte-identical to accepted
Task 381 SHA-256 `4a9f32ba633e110221c808bc4725615952f2afddee9f345caefa7db9b9f011b5`.

## Conclusion

The missing abstraction is not property typing alone. A coarse kernel can beat a fully
composed sequence of small stencils even when the latter closes more semantic coverage.
Future typed-property work must produce a rustc/LLVM-cooked coarse block/loop stencil that
keeps loaded fields and I32 values in machine registers, or price the kernel versus copied
instance in Task 157's physical cover. Guard success/failure counts are insufficient as a
cost model.
