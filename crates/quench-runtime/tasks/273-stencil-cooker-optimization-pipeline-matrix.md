# 273 — Stencil-cooker optimization-pipeline matrix

Status: complete

Measure the AOT stencil handler crate under `O2`, `O3`, `Os`, and `Oz` instead of assuming
the numerically highest optimization level is best for copied fragments. CPython's
current copy-and-patch builder deliberately chooses `-Os`: higher levels can align local
targets, tail-duplicate, or otherwise optimize each handler as a standalone function,
while the actual runtime unit is the concatenated linked image.

Keep the main runtime binary fixed. For each cooker pipeline record:

- catalog symbol, relocation, and tail-transfer validity;
- per-family code bytes and instruction counts;
- hot-path versus slow-path bytes and unintended padding;
- linked image bytes for representative numeric, property, call, and loop regions;
- semantic tests and alternating full-suite V8v7 results.

Use `STENCIL_COOKER_OPT_LEVELS`, `MAX_ALLOWED_CATALOG_GROWTH_BYTES`, and regression
thresholds as named constants in the experiment harness. Do not train LLVM with V8v7
profiles and do not choose an optimization level per benchmark or source function. One
pipeline applies to the general catalog, or a finite semantic family split is accepted
only when the family property itself explains the choice and a build-time test enforces
it.

Acceptance: a reproducible report contains all four variants; the selected pipeline
preserves extraction invariants and wins the normal alternating gate. If all produced
selected bytes are identical or performance is neutral, retain the simpler current
pipeline and close with no-op evidence.

Primary source: CPython's stencil builder and in-source `-Os` rationale
<https://github.com/python/cpython/blob/main/Tools/jit/_targets.py>; LLVM optimization
level definitions <https://llvm.org/docs/Passes.html>.

## Result

`build.rs` now has one named, validated `DEEGEN_STENCIL_OPT_LEVEL` build input over the
finite set `2 | 3 | s | z`; the accepted default is `2`. The extractor emits catalog
symbol, byte, and relocation metrics. `scripts/stencil-cooker-matrix.sh` builds isolated
variants, retains exact binaries/catalogs/logs, and rotates every valid variant through
the same suite order.

`O2` and `O3` both produce 120 valid templates with 239 supported relocations. `Os` is
rejected for a local constant-pool relocation to `lCPI110_0`. `Oz` is rejected for
cross-handler relocations to `OUTLINED_FUNCTION_5` and `OUTLINED_FUNCTION_9`. Preserving
the closed-template invariant is more important than accepting smaller standalone
objects, so neither unsupported relocation was weakened or silently copied.

`O2` reduces catalog code from 11,636 to 11,528 bytes. Exactly seven templates change:
the six region bitwise/shift operations each shrink from 61 to 57 AArch64 instructions,
and the own-property strict-equality condition shrinks from 73 to 70. The rotating
three-run screen improves from 1881.73 to 1902.39. The exact five-pair, 300 ms full-suite
comparison at `reports/task273-full-ab-5/comparison.txt` improves aggregate score from
1887.20 to **1904.63** (+0.92%); Crypto improves 12.66%, and every component clears the
-5% floor. All 94 release tests pass. The accepted binary SHA-256 is
`e3ef9dd850cf109db3f38891fe0d8794ee08842d8f53e52d0e873d5ff32c9ca4`.

The reproducible matrix, exact catalogs, rejection logs, per-symbol delta, and rotating
screen are under `reports/task273-cooker-matrix/`.
