# 214 — Numeric range and late edge-case analysis

Status: in_progress

Extend the quoted SSA/CFG facts with a finite numeric abstract domain: signed/unsigned
interval, known bits or congruence where useful, integral-vs-fractional, and explicit
`may_nan`, `may_infinite`, `may_minus_zero`, and `may_overflow_i32` flags. Run a bounded
forward fixed point before stencil tiling. This is static first-use analysis over every
eligible function, never a hotness test or benchmark selector.

Represent dense-container-relative bounds explicitly: `LengthMinus { container, offset }`
is a bound such as `array.length - 1`, not an eagerly weakened integer interval. Branch
narrowing propagates relations such as `0 <= index < array.length`; widening at loop
headers is mandatory for convergence and is controlled by named
`MAX_RELATIVE_BOUND_OFFSET` and `MAX_RANGE_FIXPOINT_ITERATIONS` constants. This lets the
same fact eliminate the lower bound, upper bound, and induction overflow checks in a
steady-state loop without a separate array-specific proof engine.

Keep semantic edge cases as obligations until code motion, folding, and dead-code
elimination finish. The final legalization pass inserts an overflow, precision, NaN, or
minus-zero guard only at a use where the distinction is observable. A proven-safe
operation selects the unchecked I32/F64 derivative from Task 149; an unresolved
obligation selects the checked derivative with a canonical generic-stencil side exit.
Bitwise `ToInt32` wrapping, division by zero, `Object.is`, reciprocal, string conversion,
and property publication are explicit observing uses.

Task 175 consumes the same range facts for trip counts and loop bounds; Task 174 supplies
reachability/constants, and Task 60 supplies the abstract-interpretation law. Keep one
range fact per SSA value and derive guard choice, representation, and diagnostics from
it. Do not store a second independent collection of "safe opcodes."

Acceptance: property tests compare checked and elided forms across integer limits,
fractional inputs, NaN, infinities, positive/negative zero, shifts, division, boxing,
and observable conversions; proofs eliminate overflow/bounds checks in representative
affine loops; unresolved cases retain correct side exits; analysis iteration and domain
width limits are named constants; disassembly plus full V8v7 A/B prove benefit.

Primary sources:
- V8 identifies numerical range analysis as a key TurboFan optimization:
  <https://v8.dev/blog/turbofan-jit>
- JavaScriptCore documents integer range optimization eliminating overflow and array
  bounds checks: <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- SpiderMonkey performs edge-case analysis late and currently uses it for negative-zero
  requirements: <https://firefox-source-docs.mozilla.org/js/MIR-optimizations/index.html>
- V8's lowering makes the precision/NaN/minus-zero obligations concrete:
  <https://chromium.googlesource.com/v8/v8/+/f0d94ede62d8e9de9ffa9365c60f4c2165270716/src/compiler/effect-control-linearizer.cc>
- Static BBV defines interval narrowing, widening, and symbolic vector-length-minus-offset
  bounds: <https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>.

## 2026-09-10 first abstract-domain slice

The first conservative fact is implemented: `GuardKind::ArrayIndex` proves that a dense
index is finite, integral, non-negative, and inside JavaScript's named maximum array-index
domain. The proof is derived from the quoted numeric region, not stored as a second list
of fast opcodes. It handles literal roots and guarded local/captured/live-in roots,
preserves the fact through move, unary plus, addition, and multiplication, and recursively
audits every write to a loop-carried local root. Subtraction, division, bitwise coercion,
dense-derived values, and any operation whose conservative maximum may leave the finite
domain reject the proof.

Proven dense sites select separate rustc/LLVM-cooked read/write templates. Those templates
still check the named JavaScript maximum and actual backing length, but omit the generic
finite, sign, and fractional checks. The generic stencil remains selected at every
unproved site; no benchmark name, source position, or runtime hotness participates.

Structural and performance evidence:

- the proven read stencil is approximately 22 AArch64 instructions versus approximately
  45 for the generic dense read;
- diagnostic runs link 43 proven-index stencils in Crypto and 74 in Navier-Stokes;
- all 97 release tests pass after moving the pure proof reducer into
  `src/numeric_region/index.rs`;
- the focused ten-pair comparison in
  `reports/task214-proven-dense-index-focused-ab-10/comparison.txt` improves the two-suite
  aggregate by 0.58%; and
- the complete five-pair comparison in
  `reports/task214-proven-dense-index-full-ab-5/comparison.txt` improves aggregate from
  1887.19 to 1897.63 (+0.55%), with every suite above the standing component floor.

This is deliberately not completion. Relative `array.length` bounds, branch narrowing,
integer congruence/known bits, overflow obligations, and late NaN/minus-zero legalization
remain.
