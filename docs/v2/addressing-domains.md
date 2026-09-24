# Bytecode addressing domains

“One fact, one representation” applies only after the fact is named correctly.
rqj's apparent addressing schemes encode three different facts, not three
copies of one fact.

## The three domains

| Domain | Question answered | Current data | Required payload |
| --- | --- | --- | --- |
| Value source | Where is this rvalue obtained? | `Operand` | register, constant, field-site, or local index |
| Receiver origin | Which object is the base of this property operation? | `FieldBase` | register, implicit `this`, or nested field-site marker |
| Variable address | Which mutable binding cell is read/written? | variable opcode + fields | local/environment slot, `(scope depth, slot)`, or global atom/cache |

The domains have different operations and effects. A value source resolves to
a value. A receiver origin is only the first step of a property access. A
variable address denotes mutable storage and may be pure, touch an environment
cell, or perform a throwing global lookup/write.

Combining them creates invalid states such as “store into a constant,” “use a
global binding as a field receiver,” or “capture a field site.” A single sum
type can describe those states in Rust, but serializing that sum into every
instruction does not remove them; it expands the representable state space and
requires every consumer to reject family-invalid variants.

## Encoding constraint

`Operand` is a 16-bit word with a two-bit tag and 14-bit payload. All four tags
are already assigned. The proposed universal address needs at least register,
constant, field, local, environment, capture, global, `this`, and nested-field
modes: nine modes, requiring four tag bits and leaving only 12 payload bits.
That cannot represent the current full 14-bit register/constant/field indexes.
Capture addresses and global atoms currently use the full 32-bit immediate;
capture specifically packs two 16-bit values.

The alternatives are all representation regressions:

- widen instruction operands, growing the fixed 12-byte instruction stream;
- move addresses to a side table, adding allocation and indirection;
- put the mode in opcode-specific spare fields, which preserves the compact
  layout but is not one canonical serialized tag.

## Measured lower bound

Task 95 implemented the narrowest favorable subset: merge only `SetField` and
`SetThisField`, where the payloads already fit `FieldBase` and no wider union is
needed. Semantics, caches, liveness, and tests passed, but the exact seven-run
Richards gate added 32 KiB median RSS at neutral Score. The candidate was
removed. A universal encoding adds more decode states and/or a larger payload;
it cannot claim a free layout win that the strict subset failed to deliver.

## Decision

Keep the three domain types separate. Share derived helpers and declarative
metadata where their knowledge is genuinely common, but do not fabricate a
universal runtime address merely because each domain contains the word
“address.” Task 94 may still test unifying the variable opcodes within the
single variable-address domain; it should not reuse `Operand` as their storage
type.
