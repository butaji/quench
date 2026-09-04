# Deegen stencil coverage inventory

This inventory is generated from the canonical `Opcode::ALL` catalog and the
build-time stencil declarations.  `DISPATCH` is the complete, semantics-
preserving trampoline region: it is a real admitted region, but it does not
claim a specialized machine-code leaf.  Specialized rows are listed
separately so the breadth claim cannot be confused with fast-path quality.

At the start of task 035 the catalog already contained 31 opcodes (the three
bounded quickened variants were added by task 032), and the generated
dispatch row used `Opcode::ALL`: catalog admission was therefore **31/31
(100%)**.  The source declaration nevertheless named only the original 28
operations.  Task 035 reconciles that declaration with the generated catalog;
the post-task admission ratio remains **31/31 (100%)**, now backed by both the
declaration and the generated `dispatch_row_covers_every_compact_opcode`
test.  The specialized-leaf ratio remains **8/31 (25.8%)**; task 040 item 2
adds bounded `CALL` and `CALL_N` bridge regions, but these intentionally reuse
the canonical call handlers rather than claiming a new callable-identity
machine-code leaf. The other entries intentionally use the complete dispatch
fallback until a profiled, proven leaf is available.

Task 042 adds three bounded sequential region rows (`LOOP_GLUE`,
`BINARY_GLUE`, and `UPDATE_RETURN`), and task 040.1 adds the measured
five-operation `ARITHMETIC_GLUE` span. These rows reuse the canonical handlers
for every operation in their generated operation slice; they are not counted
as additional specialized machine-code leaves. Full-window opcode validation
and whole-span fallback keep an Unknown or stale quickened fact from executing
a prefix of a region.

| Opcode | Covered region/stencil | Specialized leaf |
| --- | --- | --- |
| `LoadConst` | `ARITHMETIC_GLUE` (also `DISPATCH`) | No (canonical handler in bounded span) |
| `Move` | `MOVE` (also `DISPATCH`) | Yes |
| `Add` | `LOOP` / `FALLTHROUGH` (also `DISPATCH`) | Yes |
| `AddConst` | `ADD_CONST` (also `DISPATCH`) | Yes |
| `JumpIfFalse` | `DISPATCH` | No |
| `Return` | `LOOP`, `FALLTHROUGH`, arithmetic leaves (also `DISPATCH`) | Yes |
| `Slow` | `DISPATCH` | No |
| `LoadLocal` | `BINARY_GLUE` (also `DISPATCH`) | No (canonical handler in bounded span) |
| `Sub` | `SUBTRACT` (also `DISPATCH`) | Yes |
| `Mul` | `MULTIPLY` (also `DISPATCH`) | Yes |
| `Div` | `DIVIDE` (also `DISPATCH`) | Yes |
| `GetProperty` | `GET_PROPERTY` (also `DISPATCH`) | No (canonical shape-IC bridge) |
| `Call` | `CALL` (also `DISPATCH`) | No (canonical call-IC bridge) |
| `Jump` | `DISPATCH` | No |
| `IncI` | `DISPATCH` | No |
| `ForI` | `FOR_I` (also `DISPATCH`) | No (canonical structured-loop bridge) |
| `AGetI` | `GET_INDEX` (also `DISPATCH`) | No (canonical array-shape bridge) |
| `ASetI` | `SET_INDEX` (also `DISPATCH`) | No (canonical array-shape bridge) |
| `AGetIInc` | `GET_INDEX_INC` (also `DISPATCH`) | No (canonical array-shape bridge) |
| `GetN` | `PROPERTY` (also `DISPATCH`) | Yes |
| `SetN` | `SET_N` (also `DISPATCH`) | No (canonical shape-IC bridge) |
| `CallN` | `CALL_N` (also `DISPATCH`) | No (canonical named-call bridge) |
| `UpdateLocal` | `ARITHMETIC_GLUE`, `UPDATE_RETURN` (also `DISPATCH`) | No (canonical handler in bounded span) |
| `LoadLocalChecked` | `ARITHMETIC_GLUE`, `LOOP_GLUE` (also `DISPATCH`) | No (canonical handler in bounded span) |
| `Binary` | `ARITHMETIC_GLUE`, `BINARY_GLUE` (also `DISPATCH`) | No (canonical handler in bounded span) |
| `StoreLocalChecked` | `DISPATCH` | No |
| `InitLocal` | `DISPATCH` | No |
| `StoreLocal` | `ARITHMETIC_GLUE`, `LOOP_GLUE` (also `DISPATCH`) | No (canonical handler in bounded span) |
| `GetPropertyQuickened` | `DISPATCH` | No |
| `GetNQuickened` | `DISPATCH` | No |
| `AGetIQuickened` | `DISPATCH` | No |

The priority decision follows the existing neutral evidence in
`architecture-evidence.md`: arithmetic and named-property reads have measured
coverage, while calls and writes remain on the complete fallback.  No new
specialized leaf was added without a corresponding semantic proof and a
profile signal; the quickened variants are covered mechanically by the same
dispatch row.

On ARM64, task 039 now executes the same eight specialized leaves through
const-fn-generated AArch64 bytes. The generic all-opcode `DISPATCH` row remains
data-only on ARM until it receives its own ABI audit; every other catalog entry
retains the ordinary interpreter/baseline fallback.

## Task 041 scenario survey

The V8-informed cross-check is a survey of reusable JavaScript scenarios, not
an invitation to import V8's representations. Status is based on Quench's own
handlers and traces:

| Scenario | Status | Evidence and decision |
| --- | --- | --- |
| Packed versus holey/dictionary elements | **Present (canonical; bridge is guarded by fallback)** | `ArrayKind` distinguishes packed numeric/value, holey, and sparse storage. `run_compact_get_index`/`run_compact_set_index` admit dense packed access and route holes, sparse arrays, stale storage, and non-arrays through complete property semantics. The `GET_INDEX` region intentionally does not duplicate this fact; `array_region_matches_packed_and_holey_fallbacks` proves identical packed and holey results. No second elements-kind representation or stencil is justified. |
| Global variable access | **Partial** | `LoadCurrentGlobal` has a direct compact local handler, while `ResolveNameOrUndefined` remains a general slow binding operation. A Richards trace records only 3 `LoadCurrentGlobal` and 3,528 `ResolveNameOrUndefined` events against millions of ordinary compact operations (`target/richards-profile-current.json`), so there is no evidence-backed case for a dedicated global `RegionKey` yet. |
| Comparison/relational, `typeof`, `instanceof` | **Partial** | Comparisons live in the generic `Binary` family and `typeof`/`instanceof` in `Unary`/binary semantics; the arithmetic-glue region therefore does not claim a comparison-specific guard. The same Richards trace reports 3,409,018 `Binary` and 186,701 `Unary` operations, but no isolated curriculum measurement currently separates a comparison fast path from ordinary dispatch. Keep the complete generic fallback until such evidence exists. |
| `for-in` enumeration | **Present (semantic path; no stencil gap)** | Enumeration takes one key snapshot and validates membership through the canonical object-layout index, avoiding the former quadratic membership scan while preserving prototype, descriptor, deletion, and proxy behavior. `ForIn` remains a structured slow operation, and the `ForI` row is correctly only a bridge admission because it has no bytecode back-edge. An enum-cache stencil would duplicate mutation-sensitive semantics without a measured benefit. |
| String concatenation representation | **Absent (explicitly deferred)** | Curriculum case 032 remains a measured string-heavy wall-time outlier (27.41x in the current 38-case trace sweep). This is a rope/cons-string representation issue, not stencil admission; it needs a separate `value.rs`/string-representation task and is not changed here. |

The only correctness-sensitive check in this survey is the packed/holey array
pair above. All other categories remain complete ordinary semantics with no
new speculative or representation-specific fast path.
