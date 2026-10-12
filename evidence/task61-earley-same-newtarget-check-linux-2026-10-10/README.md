# EarleyBoyer same-target constructor check elimination (2026-10-10)

The scoped EarleyBoyer profile recorded 8,963,440 `Construct` dispatches. For
`new C()`, Quench checked constructability once for `C` and immediately checked
the identical value again as `newTarget`. I skipped the second check only when
the two runtime values are equal; the separate `Reflect.construct` target
check remains in place.

Node v24.19.0, the accepted binary, and the candidate matched on normal,
bound, proxy, and derived-class construction, primitive/object constructor
returns, same/different `newTarget`, and invalid constructors. EarleyBoyer
output matched in all three alternating production pairs, but Score deltas
were −2, −1, and −10. Candidate wall time was higher in all three pairs.
Median maximum RSS fell by 61,440 bytes, too small to justify the consistent
Score regression; the source change was removed.

Baseline binary SHA-256:
`ef44efdbab5c74177f23b610c72e65395a97f42627c5d9e088f9c7b757d8339f`.
Candidate binary SHA-256:
`f6dc3114522f4ffb59a088a16af211cbca168bd737f319f2c8860d93698ea862`.
Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

The rejection screen is [screen-3.json](screen-3.json); the source patch,
paired runner, Node oracle, and raw outputs are preserved alongside it. This
three-pair screen is diagnostic, not Stage B qualification.
