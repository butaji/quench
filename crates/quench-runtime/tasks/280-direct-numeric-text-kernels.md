# 280 — Direct numeric parse/format kernels over borrowed string views

Status: in_progress

Replace allocation-heavy numeric text conversions with one quoted `NumericTextRecipe`
sum interpreted by generic semantics and typed kernels:

- `ParseInteger { width, radix_mode, trailing_junk }`;
- `FormatInteger { radix }`;
- `FormatShortestDouble`; and
- the existing fixed/precision modes.

The first slice handles `parseInt` and integral `Number.prototype.toString(radix)` over a
borrowed flat one-byte string view. Scan leading whitespace/sign/prefix and digits in
place; return NaN when no digit is accepted; preserve negative zero and rounding rules.
Use shift/mask accumulation for proved power-of-two radices and checked multiply/add for
other radices. Format integers backward into a named fixed-capacity stack buffer and
allocate only the exact final JS string. Single-character and bounded small-integer
results may reference Task 198's immutable kernel heap/Task 230's bounded atom cache.

Task 190 later derives one-byte and two-byte kernels from the same recipe and supplies an
explicit flatten edge for ropes. Decimal floating conversion should first reuse Rust's
verified standard implementation; evaluate a different shortest-roundtrip algorithm
only with semantic and A/B evidence. Do not add a formatting dependency merely because
V8 uses Dragonbox.

Builtin identity and effect facts remain solely in Task 183's `BuiltinRecipe`; this task
adds conversion algorithms, not another builtin table. V8v7 source occurrences in
Crypto, Earley-Boyer, and Splay justify measurement but never select a variant.

Use named `INTEGER_TEXT_STACK_CAPACITY`, `MIN_NUMERIC_RADIX`,
`MAX_NUMERIC_RADIX`, and cache-bound constants. Avoid all representation/layout magic
numbers.

Acceptance: differential tests cover every radix, whitespace, signs, prefixes, trailing
junk, no digits, overflow rounding, NaN/infinity, positive/negative zero, fractions,
Latin-1/UTF-16, and rope flattening; allocation counters prove one final allocation on
the flat fast path; direct-kernel counters show real selection; Crypto/Earley-Boyer/Splay
and full V8v7 alternating A/B improve.

Primary sources:

- V8 direct one-byte/two-byte integer parsing and power-of-two specialization:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/numbers/conversions.cc>
- V8 number-to-radix builtin fast cases:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/builtins/builtins-number.cc>
- V8 shortest-double and segmented-buffer report:
  <https://v8.dev/blog/json-stringify>

## 2026-09-10 first-slice experiment

Implemented and rejected an allocation-minimized semantic-kernel slice. It scanned
borrowed `Rc<String>` storage directly, fast-pathed number inputs only inside the
fixed-decimal `Number::toString` range, preserved negative zero and no-digit NaN, and
formatted integral radix output backward through a named 65-byte stack buffer with one
final string allocation. All 100 release tests passed, including radix, prefix,
whitespace, trailing-junk, exponent-boundary, and format cases.

The ten-pair selected-suite comparison at
`reports/task280-numeric-text-focused-ab-10/comparison.txt` regressed Crypto 1.11% and
Earley-Boyer 0.24%, with Splay neutral; aggregate changed 2264.60 to 2254.69 (-0.44%).
The candidate SHA-256 was
`18e7165c10e1590fade521957407bd0443bf6deefd80a5fa39c217d0c48d2698` and the slice was
removed. This task remains in progress, but the next attempt must first erase/specialize
the generic builtin call boundary and measure actual recipe selection. A faster inner
conversion algorithm alone is below the current end-to-end noise floor.

## Round-forty-three proved algorithm candidates

Once Task 183/401 erases the generic builtin-call seam, compare two algorithms behind the
existing `NumericTextRecipe` rather than adding another representation. `FormatShortestDouble`
may use Ryū's fixed-size-integer shortest-roundtrip conversion. Decimal parsing may use the
Eisel–Lemire common path with its exact fallback for ambiguous/out-of-range inputs. Both must
preserve ECMAScript formatting thresholds, `NaN`, infinities, negative zero, exponent spelling,
and round-to-nearest-even. Keep Rust's canonical conversion as the differential oracle and
fallback until exhaustive bit-pattern and grammar tests pass. No new dependency or algorithm
is justified before direct recipe counters show the conversion body, rather than its call and
allocation boundary, is material.

Sources: Ryū <https://doi.org/10.1145/3192366.3192369> and Eisel–Lemire
<https://arxiv.org/abs/2101.11408>.
