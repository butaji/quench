# 381 — Direct dense-computed stencil projection

Status: complete

Implement the highest-value independently buildable family from the post-Task378
residual frontier: general dense numeric `GetComputed` and `SetComputed` leaves that run
inside maximal composed blocks. Keep `ArrayStorage` as the owned semantic representation;
publish only a fixed-layout projection that AOT code can consume. No suite identity,
source position, runtime hotness, or observed value is allowed to select the templates.

## Quoted data and invariants

`Object` now contains a C-layout `DenseArrayAccess { elements, length }` immediately after
its prototype word. `stencil-aot/object_layout.rs` is the single named offset vocabulary
for both the runtime and cooker. Compile-time assertions bind the Rust layout to those
word offsets, and the build script explicitly tracks the shared schema as an input.

The owned `Option<ArrayStorage>` remains authoritative. `ObjectCell::new`, reuse, and the
`ObjectBorrowMut` effect boundary derive the projection from it. Ordinary objects publish
a null elements pointer. This follows the Lisp staging rule: one canonical structure,
one derived AOT view, and mutation confined to a named boundary.

The cooked load leaf accepts only:

- an object-tagged receiver with a non-null dense projection;
- a finite, non-negative integral JavaScript array index within the existing length;
- source and destination values that need no reference-count ownership operation.

The cooked store adds one invariant: replacing the slot must preserve its numeric versus
non-numeric class, so `ArrayStorage::non_number_count` remains correct. Extension,
prototype fallback, string/function/regexp ownership, and representation changes take the
existing per-op semantic slow edge. The hit path performs no Rust callback and tail-calls
the next stencil through the standard connector.

## Verification and measurement

- 144 release tests pass. New tests execute both extracted machine-code leaves, verify an
  existing dense load/store, and verify the out-of-bounds slow transfer. Structural
  selection tests now treat computed operations as composable primitive morphisms.
- The candidate binary is `/tmp/deegen-task381-dense-computed-candidate`, sha256
  `4a9f32ba633e110221c808bc4725615952f2afddee9f345caefa7db9b9f011b5`.
- The three-pair screen in `reports/task381-dense-computed-ab-3/comparison.txt` measured
  +0.63% aggregate.
- The prescribed nine-pair exact comparison in
  `reports/task381-dense-computed-exact-ab-9/comparison.md` measured 2262.27 -> 2297.84,
  **+1.57%**, with a 95% paired-bootstrap interval of **[+0.03%, +3.36%]**. Richards
  improved 3.23% `[+2.56%, +3.91%]`; Earley-Boyer improved 2.08%
  `[+0.52%, +4.52%]`; no suite crossed the regression floor. The gate passed.
- Differential cooker audits pass at both `-O2` and `-O3` in
  `reports/task381-stencil-cooker-audit/`: 154 catalog symbols, 46
  placeholder-sensitive stencils, and 88 typed holes, with deterministic A/B objects and
  confined variant-C changes.
- The first exact structural record grows direct coverage from 2,030 blocks / 6,811
  opcodes to 2,195 blocks / 8,733 opcodes. Copied function code grows from 719,828 to
  825,704 bytes. `otool` on both extracted handlers shows only inline loads, guards,
  indexed word access, stores, and relocatable tail branches—no `bl` helper call.

## Rejected complementary experiment

The refreshed frontier suggested that bitwise plus computed closure was larger than
computed alone. Six binary bitwise/shift leaves and three numeric-unary leaves were
generated with Rust macros and tested as one follow-up catalog extension. The short
three-pair comparison in `reports/task381-computed-bitwise-ab-3/comparison.txt` measured
+1.14%, but the required exact comparison in
`reports/task381-computed-bitwise-exact-ab-9/comparison.md` measured -3.45%, interval
`[-8.65%, +0.49%]`; Crypto, RayTrace, and Earley-Boyer breached the suite floor. The
extension was reverted while its binary and reports were retained. Future integer work
must carry an I32 context across a region instead of paying standalone double-to-int and
int-to-double conversions in large per-op leaves.
