# 358 — Traced-object native transfer edges

Status: complete

Remove a stale ownership guard from every rustc/LLVM-cooked value-transfer stencil. The
runtime migrated ordinary objects from `Rc<RefCell<Object>>` ownership to non-owning,
one-word `ObjectHandle`s managed by the tracing `ObjectHeap`, but the AOT handler source
still treated every tag at or above `STRING_TAG` as reference-counted. Consequently an
object in a source or destination forced local/register moves, property-result transfers,
numeric destination overwrites, terminal returns, and fused recurrence blocks to the
generic Rust slow edge even though copying or replacing an object handle has no retain or
release effect.

Define the real predicate once in the AOT semantic source:

`needs_reference_count(value) = tag >= STRING_TAG && tag != OBJECT_TAG`.

The expression follows directly from the canonical contiguous tag layout: strings,
functions, and regexps retain Rust `Rc` ownership; ordinary objects are the only tracing-
heap tag in that range. Constants remain named, and every existing stencil continues to
send reference-counted sources/destinations to the canonical ownership-aware slow kernel.
No benchmark identity, source pattern, runtime heat, or second execution representation is
introduced.

The immediate evidence is a current Richards native profile: 55.01% of samples are in
`dyn_block_step_impl`, while only 2.36% are in the call-IC entry itself. A complete residual
block census is retained under `reports/task358-current-residual-census/`; it confirms that
property-heavy blocks dominate Richards, DeltaBlue, Earley-Boyer, RayTrace, and Splay.

Acceptance:

- an executable cooked-stencil test proves an object move takes the native continuation
  while a string takes the reference-counted slow continuation;
- all release tests and the complete V8v7 suite pass;
- retain only if an interleaved full-suite A/B clears the standing component and aggregate
  floors; otherwise revert the semantic change and record the negative reach evidence.

## Result

Accepted. The executable test distinguishes the two continuations and proves that a
tracing-heap object handle copies directly while an `Rc<String>` still takes the ownership
slow edge. `cargo test --release -- --test-threads=1` passes all **130 tests**.

Three alternating 200 ms complete-suite repetitions against the accepted Task 353/357
baseline measure:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 923 | 933 | +1.08% |
| DeltaBlue | 923 | 941 | +1.95% |
| Crypto | 1798 | 1793 | -0.28% |
| RayTrace | 1961 | 1971 | +0.51% |
| Earley-Boyer | 2768 | 2779 | +0.40% |
| RegExp | 3720 | 3703 | -0.46% |
| Splay | 4053 | 4035 | -0.44% |
| Navier-Stokes | 6968 | 6947 | -0.30% |
| **Geometric aggregate** | **2331.62** | **2338.72** | **+0.30%** |

Raw artifacts are in `reports/task358-traced-object-native-transfer-ab-200ms-3/`.
Candidate SHA-256:
`76a463a6cccf7bf37f83b842a97758de5551cb031b09704033a600b3b01d0a04`.

This is a representation-law repair, not the large structural win. The current Richards
sample and residual census still select the same next target: replace dominant mixed
property/call blocks in `dyn_block_step_impl` with closed shape/slot/call morphisms.

