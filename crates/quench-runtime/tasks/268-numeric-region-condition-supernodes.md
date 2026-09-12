# 268 — Numeric-region condition supernodes

Status: complete

Tile proven-number loop conditions as one coarse categorical morphism:

- `ReadLocal ; ReadLocal ; Compare ; JumpIfFalse`
- `ReadLocal ; ReadCaptured ; Compare ; JumpIfFalse`
- `ReadLocal ; NumberLiteral ; Compare ; JumpIfFalse`

Rust macros generate the finite equality and ordering families. The numeric-region entry
guard owns type validation; these internal templates load the two operands directly,
compare in LLVM-selected registers, and branch without materializing either operand or
the dead Boolean result in the virtual register array. The ordinary true and false
symbolic continuations remain the Lego connectors.

Selection is a pure quote-stage pattern over consecutive `RegionOp`s. It requires exact
def/use closure for all three temporary registers and rejects every control edge into an
interior operation. Captured values use the effect-refreshed snapshot already owned by
the region. No property name, source location, benchmark identity, runtime counter, or
hotness state participates.

Acceptance: macros and catalog tests cover all operand/compare families; selector tests
cover positive and negative cases; linked statistics prove selection in Crypto and
Navier-Stokes;  release tests and complete smoke pass; alternating affected-suite and
complete-suite A/B decide acceptance.

Sources: Copy-and-Patch supernodes <https://arxiv.org/abs/2011.13127>, lazy block
versioning's type-test elimination <https://arxiv.org/abs/1411.0352>, and V8 Liftoff's
register-cache state <https://v8.dev/blog/liftoff>.

Current CPython copy-and-patch work independently confirms the exact representation win:
keep comparison output as a one-bit predicate and let the following branch consume it,
rather than load/materialize a language Boolean and compare it again. Task 271 will
generalize this accepted family into a composable predicate context instead of adding a
new fused producer/branch stencil for every future condition kind. Source:
<https://github.com/python/cpython/issues/149238>.

## Result

Implemented 18 rustc/LLVM-cooked templates: three right-operand families (local,
captured-name snapshot, and numeric literal) crossed with all six equality/ordering
operators. The RegionPlan selector enforces exact temporary liveness and rejects every
interior control-flow entry. The linked image patches separate true and false symbolic
continuations, so the condition never materializes a JavaScript Boolean.

All 94 release tests pass. Execution tests cover every family/operator pair, both exits,
and JavaScript's unordered `NaN` cases. Complete-suite smoke passes. Runtime diagnostics
prove this is executed rather than merely catalogued: Crypto links 8 condition
supernodes and Navier-Stokes links 20 in the recorded 50 ms runs.

The ten-pair, 200 ms affected-suite comparison at
`reports/task268-focused-ab-10/comparison.txt` improves the geometric mean by 0.97%
(Crypto +0.34%, Navier-Stokes +1.62%). The five-pair, 300 ms complete comparison at
`reports/task268-full-ab-5/comparison.txt` is neutral at -0.08%; the affected medians
remain positive (Crypto +0.97%, Navier-Stokes +0.91%) and every suite clears the -5%
component floor. The exact candidate SHA-256 is
`0d3ba9ff9f91dde5dee67afd0fe23300e699039bac4153046c24de98434138d7`.

This is retained as a proven representation/coverage improvement, not counted as an
aggregate-score breakthrough. Task 271 generalizes the result into one predicate
producer/consumer algebra; Task 272 prevents further fused families from becoming a
hand-maintained Cartesian product.
