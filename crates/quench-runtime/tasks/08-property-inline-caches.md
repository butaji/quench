# 08 — Shape-guarded property IC stencils

Status: complete

Static get/set sites carry IC data containing a receiver shape id, prototype guards when required, and a fixed slot offset. Hit stencils perform guard plus direct slot load/store. Miss kernels perform semantic lookup and update the site without duplicating property semantics.

Begin monomorphic; add a bounded polymorphic form only from measured evidence. Own-property and inherited-property cache rules remain distinct. Existing slot-only `PropertyIc` is not sufficient because it does not guard a hidden class.

Acceptance: mutation/prototype invalidation tests, no stale inherited hit, and property-heavy benchmark gains.

Current state: static own-property cache hits guard `receiver_shape_id` and load/store a
fixed slot without comparing the property string. Tests cover own shadowing, same-shape
reuse, and rejection of a different key order. The fast path still lives inside the
shared Rust opcode kernel; task 36 moves it into executable stencils.

Current experiment: add a separate inherited-property cache without enlarging the
monomorphic own-hit cell. It records the receiver shape plus each prototype object's
identity and immutable shape through the holder slot. Validation walks only this fixed
guard chain and reads the current holder value; receiver/intermediate shadowing,
deletion, holder-layout mutation, or prototype-link replacement invalidates naturally.
This directly targets the 243,605 Richards and 538,017 DeltaBlue static misses measured
during the rejected Task 31 own-property PIC experiment.

## Result: accepted

Static property sites now keep the original compact monomorphic own-property cell plus
a separate inherited-property guard chain. An inherited entry records receiver shape,
each prototype identity and immutable shape, and the holder slot. Hits read the current
holder value without hashing the key. Any receiver/intermediate shadowing, deletion,
shape transition, or prototype-link replacement fails validation and refills through
the canonical lookup. Focused tests cover own shadowing, same/different layouts,
intermediate prototype shadowing, and prototype replacement.

Disabled-by-default counters prove this is the miss population that matters:

- Richards: 258,372 inherited hits, 24,849 misses, 24,888 fills;
- DeltaBlue: 424,247 inherited hits, 8,997 misses, 9,098 fills.

The initial 20 ms full run was invalidated by severe host noise. A stable four-run,
100 ms/minimum-five alternating full-suite comparison in
`reports/task08-prototype-chain-ic-stable-full-ab-4/comparison.txt` passes every gate:
aggregate 1010.83 → 1043.21 (+3.20%), Richards +13.85%, DeltaBlue +12.89%, and every
other suite between −1.00% and +1.58%. The executable is preserved as
`/tmp/deegen-task08-prototype-chain-ic`, SHA-256
`b8783c2f28caac3aa8cf5c6196090e1788fd14ba95e212b33e26f66056e34e56`.

This completes property-IC semantics. Moving the hit path from the shared Rust kernel
into native AOT stencil regions remains separately tracked by [[36-direct-opcode-stencils]].
