# 160 — Own-property nullish condition stencils

Status: complete

Add one general macro-generated direct block family for:

`LoadLocal(receiver); GetStatic; LoadLiteral(null|undefined); Compare; JumpIfFalse`

The property result and comparison temporaries must be dead after the block. On an own
monomorphic property-IC hit, the AOT stencil loads the fixed slot and performs loose
nullish or strict immediate comparison without materializing intermediate virtual
registers, cloning heap values, calling a Rust helper, or decoding bytecodes. An empty,
inherited, or mismatching IC exits through the canonical whole-block slow stencil, which
also populates the ordinary cache.

This is a finite IC-slab vocabulary experiment toward Task 145, not completion of its
mutable slab publication infrastructure. Selection depends only on bytecode structure,
liveness, literal kind, and comparison semantics. Named constants define every site
offset and instruction count.

Evidence: the Task 151 Richards residual profile records 25,032
`GetStatic + null + Eq` block entries and 83,172 `GetStatic + null + Ne` entries in a
20 ms run. The previous numeric property-condition family in Task 138 regressed because
it repeated both shape and numeric/coercion guards; nullish identity needs only the
already-required shape guard and tag comparison.

Acceptance: structural/liveness selection tests; semantic tests for null, undefined,
number, object, inherited property, shape mismatch, loose and strict equality; complete
release tests and V8v7 smoke; alternating A/B against the exact Task 150 binary; accept
only if the full aggregate improves without component-floor failures.

## Result

Accepted. Four rustc/LLVM-cooked templates are generated from one Rust macro: loose
nullish equal/not-equal and strict null/undefined equal/not-equal. The selector first
parses the five-op semantic form into `PropertyLiteralCondition` data, then separately
checks liveness and maps the literal/comparison pair to a template. All positions use
named constants.

The property slot is borrowed as a raw word on an own shape-cache hit. No intermediate
virtual register is written and no heap value is retained or released. Cache-empty,
shape-mismatch, inherited-property, and non-object cases branch to the canonical whole-
block slow adapter. Tests explicitly cover all four templates, liveness rejection,
shape mismatch, and inherited lookup.

Evidence:

- 80 release tests pass.
- Complete smoke: `reports/task160-property-nullish-smoke.jsonl`.
- Targeted ten-pair, 500 ms Richards comparison:
  `reports/task160-property-nullish-richards-ab-10/comparison.txt`, 646.5 to 703.5
  (**+8.82%**).
- Full six-pair, 500 ms comparison:
  `reports/task160-property-nullish-full-ab-6/comparison.txt`, 1682.38 to 1713.28
  (**+1.84%**). Richards improves **10.00%** and every component clears the standing
  floor.
- Accepted binary: `/tmp/deegen-task160-property-nullish`, SHA-256
  `acde4028db7f7ce91b906778fc1b8e5ef041e3e665ed58694074d3e5fcce0f6c`.

This validates direct property predicate stencils but does not complete Task 145: the
cache fields are still data loaded by the template, rather than code patched into a
reserved executable slab.
