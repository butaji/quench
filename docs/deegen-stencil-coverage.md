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
test.  The specialized-leaf ratio is **8/31 (25.8%)**; the other entries
intentionally use the complete dispatch fallback until a profiled, proven
leaf is available.

| Opcode | Covered region/stencil | Specialized leaf |
| --- | --- | --- |
| `LoadConst` | `DISPATCH` | No |
| `Move` | `MOVE` (also `DISPATCH`) | Yes |
| `Add` | `LOOP` / `FALLTHROUGH` (also `DISPATCH`) | Yes |
| `AddConst` | `ADD_CONST` (also `DISPATCH`) | Yes |
| `JumpIfFalse` | `DISPATCH` | No |
| `Return` | `LOOP`, `FALLTHROUGH`, arithmetic leaves (also `DISPATCH`) | Yes |
| `Slow` | `DISPATCH` | No |
| `LoadLocal` | `DISPATCH` | No |
| `Sub` | `SUBTRACT` (also `DISPATCH`) | Yes |
| `Mul` | `MULTIPLY` (also `DISPATCH`) | Yes |
| `Div` | `DIVIDE` (also `DISPATCH`) | Yes |
| `GetProperty` | `DISPATCH` | No |
| `Call` | `DISPATCH` | No |
| `Jump` | `DISPATCH` | No |
| `IncI` | `DISPATCH` | No |
| `ForI` | `DISPATCH` | No |
| `AGetI` | `DISPATCH` | No |
| `ASetI` | `DISPATCH` | No |
| `AGetIInc` | `DISPATCH` | No |
| `GetN` | `PROPERTY` (also `DISPATCH`) | Yes |
| `SetN` | `DISPATCH` | No |
| `CallN` | `DISPATCH` | No |
| `UpdateLocal` | `DISPATCH` | No |
| `LoadLocalChecked` | `DISPATCH` | No |
| `Binary` | `DISPATCH` | No |
| `StoreLocalChecked` | `DISPATCH` | No |
| `InitLocal` | `DISPATCH` | No |
| `StoreLocal` | `DISPATCH` | No |
| `GetPropertyQuickened` | `DISPATCH` | No |
| `GetNQuickened` | `DISPATCH` | No |
| `AGetIQuickened` | `DISPATCH` | No |

The priority decision follows the existing neutral evidence in
`architecture-evidence.md`: arithmetic and named-property reads have measured
coverage, while calls and writes remain on the complete fallback.  No new
specialized leaf was added without a corresponding semantic proof and a
profile signal; the quickened variants are covered mechanically by the same
dispatch row.
