# 11 — Interned keys and efficient strings

Status: planned

Introduce canonical interned symbols for identifiers and property keys so hot property operations compare compact identities, not allocated `String` values. Separate key identity from general JS string contents. Evaluate inline short strings or ropes only after profiles identify general string allocation as material.

Small-integer keys (array-index property access, `arr[0]`, loop-indexed access) should
never round-trip through the string interner at all. QuickJS tags small integers
directly into its atom representation (`JS_ATOM_TAG_INT`, `quickjs.c:2870-2898`) so a
numeric property key is a masked integer compare, not a string allocation or a hash
lookup into the intern table. Reserve a tag bit in this VM's key representation so a
small non-negative integer key encodes directly (no interning, no allocation) and
compares by value, while every other key goes through the canonical interned-symbol
table; the guarded property/array-access connectors from [[08-property-inline-caches]]
and [[32-element-kind-guarded-arrays]] should be able to treat a tagged-integer key and
an interned-symbol key uniformly at the guard-dispatch level.

Acceptance: key interning has one canonical table and no semantic dependence on pointer identity for non-key strings; concatenation/template/property-key correctness tests; measured allocation reduction; a small-integer property/array-index key produces zero string-interner allocations or lookups, verified by an allocation counter, while remaining correctly convertible to its string form where the language requires it (e.g. `for-in` key enumeration, `String(key)`).
