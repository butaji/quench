# 116 — Compact 32-byte inline-site layout

Status: complete

Replace the 56-byte `InlineSite`/AOT ABI record (six machine-word metadata fields plus one
literal) with six explicit 32-bit metadata fields and one 64-bit literal, for a 32-byte
power-of-two stride. Program counters are already capped by `MAX_EMBEDDED_PC`, registers
are `u16`, opcodes fit in `u32`, and local-slot conversion is checked while constructing
the site table. Malformed metadata rejects native compilation rather than truncating.

This is a representation-level monoid improvement shared by every abstraction level:
opcode, block, loop, function, and program stencils all consume the same immutable site
sequence. A 32-byte stride reduces memory by 43% and lets LLVM form `site + index << 5`
instead of multiplying an index by 56 when resolving bytecode continuations. It changes
no semantic selection, hotness policy, or JS-source behavior.

Acceptance: Rust/AOT layout size and field-offset assertions agree; oversized synthetic
local operands reject site construction; all release tests and full smoke pass; representative
cooked stencils shrink or improve their address-generation instruction sequence; an exact
alternating six-run full-suite A/B against [[110-constant-condition-control-stencils]]
improves aggregate without crossing the standing -5% per-suite floor. Revert and record
negative evidence otherwise.

## Implementation tested

- Changed the shared Rust runtime and `no_std` AOT `InlineSite` ABI from six
  machine-word metadata fields plus a `u64` literal (56 bytes on AArch64) to six
  `u32` metadata fields plus the literal (32 bytes).
- Derived the record size and literal offset from named field-count/type constants;
  matching compile-time size/offset assertions guarded both ABI definitions.
- Kept malformed bytecode safe: site construction used checked `usize -> u32`
  conversion and rejected an unrepresentable local slot instead of truncating it.
- Applied the representation uniformly. Opcode, block, loop, function, and program
  stencil selection were unchanged and consumed the same compact immutable table.

## Verification

- `cargo test --release -q`: 47 passed, 0 failed.
- Full V8v7 smoke: all eight suites valid (`479, 444, 1036, 1038, 1618, 239,
  1872, 1239` in suite order).
- `deegen_dyn_dead_recurrence_add_less` disassembly changed continuation lookup to
  `add x1, x8, x9, lsl #5`, proving LLVM used the power-of-two 32-byte stride.
- Representative cooked code sizes did not shrink: dead condition 96 bytes,
  recurrence 140 bytes, update 112 bytes, return-local 80 bytes. The benefit was
  limited to metadata footprint and continuation address generation.

## Exact A/B decision

Report: `reports/compact-inline-site-ab-6/comparison.txt`

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 493.5 | 498.5 | +1.01% |
| DeltaBlue | 453.5 | 447.5 | -1.32% |
| Crypto | 1047 | 1053 | +0.57% |
| RayTrace | 1107.5 | 1092 | -1.40% |
| EarleyBoyer | 1665 | 1649 | -0.96% |
| RegExp | 224 | 231 | +3.12% |
| Splay | 2006.5 | 1981 | -1.27% |
| NavierStokes | 1236.5 | 1227.5 | -0.73% |
| **Aggregate** | **836.674** | **835.566** | **-0.13%** |

Candidate SHA-256: `495ff45bc5f4009bd2ad7d4d5e12344f4b30360cbdffff367ff0380319d6f8a6`.
The sixth repetition was visibly system-throttled for both sides; the harness's
per-suite median excluded those low outliers from the reported comparison.

The aggregate failed the standing positive-improvement gate, so the compact ABI was
rejected and fully reverted. Rebuilding after the revert reproduced the accepted
Task 110 binary byte-for-byte:
`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`.

## Learned constraint

Shrinking cold per-function site metadata is not enough at the current scale. The
remaining performance work must remove repeated semantic guards, loads, and connector
transitions from executed stencils; representation wins that leave cooked stencil
bodies effectively unchanged are below benchmark noise.
