# Object and string sizes

Byte counts for the heap's small objects and ASCII strings, derived by reading the declared
layouts (asserted `size_of` values in the sources), not by measurement. No build or run was
made for this document. Allocation sizes assume the project allocator `quench-alloc`: 16-byte
granule size classes (16, 32, ..., 256, 320, ...), no per-block header.

## A. Ordinary object property storage

Record: `Object` is 12 bytes (`payload: u64` + `layout: u32`, `repr(C, packed(4))`,
`heap/cell.rs`). Two properties fit the payload word as narrow values (`Value::to_narrow`,
two 32-bit halves, `INLINE_PROPERTY_COUNT = 2` in `value_vec.rs`). Narrow integers cover
30-bit signed values (`NARROW_INTEGER_BITS`), plus null and heap references in narrow families.
Larger properties use an arena run: a narrow run takes 4 bytes per slot (two per word), a wide
run takes 8. Capacities are powers of two starting at `MIN_CAPACITY = 4`. Arena runs carry no
per-run header; the arena's own growth slack is amortised and not counted.

Own data properties on a plain, non-array object:

| Props | Narrow values (record + arena) | Wide values, arena (record + arena) | Wide values, constructed inline (record + `ObjectExtras` box) |
|------:|-------------------------------:|------------------------------------:|---------------------------------------------------------------:|
| 0 | 12 | 12 | 12 |
| 1 | 12 | 12 + 32 (cap 4) | 12 + 64 = 76 |
| 2 | 12 | 12 + 32 (cap 4) | 12 + 64 = 76 |
| 3 | 12 + 16 (cap 4, narrow) = 28 | 12 + 32 = 44 | 12 + 64 = 76 (if still inline) |
| 4 | 28 | 44 | 76 |
| 5-8 | 12 + 32 (cap 8, narrow) = 44 | 12 + 64 = 76 | 76 |

Notes:
- The common small object (0-2 properties, int/null/reference values) is 12 bytes and has no
  out-of-line allocation. Inline storage already exists and is the default.
- `ObjectExtras` is 64 bytes (`repr(C)`, derived offsets in `heap/cell.rs`). It holds the inline
  values once a wide value has to live in the record (`set_inline_property_wide`,
  `store_inline_values`, and `with_property_storage` when inline values are not narrow). Its
  box costs a 64-byte class under the granule allocator.
- The store path (`heap.rs` `property_set` / `widen_inline_property`) moves a wide value into an
  arena run of capacity 4 (32 bytes) instead of extras. The extras box therefore comes mainly
  from constructors that receive wide inline values directly. Saving: 76 - 44 = 32 bytes per
  such two-property object.
- Reordering `ObjectExtras` fields to reach 56 bytes would not help: the granule allocator puts
  56 and 64 bytes both in the 64-byte class. A 48-byte target needs about 16 bytes removed (the
  16-byte `Watch` is the largest).

## B. Strings

Representation (`vm/wtf16.rs`, `vm/wtf16/payload.rs`, `heap/small_strings.rs`):
- `JsString` is 24 bytes (asserted, `FlatUnits` is three words). Every flat string has a 24-byte
  string record.
- ASCII is always narrow (one byte per unit). `FlatUnits::Short` holds up to
  `SHORT_ASCII_CAPACITY = 22` bytes inside the record. Longer ASCII uses `Rc<str>` (16-byte
  header plus bytes), with `AsciiSlice` for shared tails.
- Non-ASCII text uses UTF-16 (`Rc<[u16]>`, two bytes per unit), including Latin-1 characters
  0x80-0xFF.
- Single-unit ASCII strings share one record per unit (`ascii_unit_strings`).
- Ropes: `JsStringInner` is about 40 bytes plus its 16-byte `Rc` header (64-byte class), with
  hashes computed lazily in padding (`compare_hash`), so unhashed strings pay nothing extra.
- Concatenations of parts before their text is written use a 12-byte `SmallParts` record.

| ASCII length | Record | Text | Total (approx.) |
|-------------:|-------:|-----:|----------------:|
| 1 | 24 (shared per unit) | 0 | about 0 per string |
| 8 | 24 | 0 (`Short`, inline) | 24 |
| 32 | 24 | `Rc<str>` 16 + 32 = 48 (48-byte class) | 72 |
| 200 | 24 | `Rc<str>` 16 + 200 = 216 (224-byte class) | 248 |

Non-ASCII comparison (same length): 200 Latin-1 units as `Rc<[u16]>`: 16 + 400 = 416 (432-byte
class) plus 24 = 456 bytes, against 248 for ASCII.

## Candidate wins, not implemented

1. Latin-1 storage for non-ASCII text below U+0100 (one byte per unit). For accented text this
   halves the text bytes, about 208 bytes saved at length 200. This needs a new `Repr` variant
   and touches conversions, search, regexp subjects and the JIT string paths. Too large to
   verify without a build.
2. Constructors that receive wide inline values should build an arena run of capacity 4
   (32 bytes) instead of a 64-byte `ObjectExtras` box, saving 32 bytes per such object. Needs
   the arena threaded through the construction path (`with_property_storage` and the planned
   constructors). Not implemented because the storage-form invariants (`value_vec.rs`
   `INLINE_FORM` / `EXTRAS_FORM` / `NARROW_FORM`, the JIT stencil literals in
   `quench-bjit/stencils/layout.rs`) cannot be checked without running `stencil_layout_matches`.
3. Rc header for ASCII strings of 23 bytes or more costs 16 bytes per string. A `Box<str>` would
   save it, but shared slices depend on `Rc`, so this is a larger change.
