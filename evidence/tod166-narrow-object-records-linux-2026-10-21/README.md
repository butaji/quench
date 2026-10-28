# TOD-166: 16-byte object records with two narrow inline slots

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`.

- Base `b166`: v2-gc at bc15d34fd.
- Candidate `c166d`: `cand-gc-narrow-objects` at the commit that restores the two-value
  arena minimum.
- Both run with `QUENCH_COMPILE_CACHE=` (see "Method" below).

## Change

- An `Object` is its 8-byte header plus one payload word, 16 bytes instead of 24. The
  payload word holds one of:
  - the two inline property slots as narrow 32-bit values (`Value::to_narrow`)
  - the arena position
  - the `ObjectExtras` pointer
- A narrow value is either:
  - a 30-bit integer, or
  - a 4-bit code naming the value's upper word (eight heap reference families, undefined,
    null, booleans, the hole) plus a 25-bit lower word.

  Decoding rebuilds the exact bits, so a value is narrow exactly when it round-trips. The
  top two bits of a narrow value are equal, so a payload word of narrow values never
  carries the extras tag.
- Storing a value that has no narrow form (a double, or a reference past 2^25) moves the
  object's values to the arena, the same storage an object uses past two properties. An
  object with extras keeps full-width values there.
- `ArrayObject` holds its element backing as its own field, so array records stay 24 bytes.
- Hot paths convert each value once: field adds and sets (`store_inline_narrow`), planned
  construction and object literals.

## Result: kept

Rule, written on TOD-166 before measuring:
- RSS geomean ≤ ×0.99 on V8-v7, and no suite's RSS above ×1.005.
- No suite's speed below ×0.99.
- npm top 300: RSS ≤ ×1.0, no package failing that passed before.
- Unit tests pass.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 10 | ×1.007 | **×0.962** (splay 38.3 → 31.4 MB, earley-boyer 8.2 → 7.1) |
| JetStream (12 GC-heavy tests) | 5 | ×0.999 | **×0.972** (doxbee 54.6 → 51.1, espree 52.6 → 49.8, acorn 38.6 → 36.8, hash-map 25.8 → 24.2) |
| Frameworks | 5 | ×1.010 | ×0.995 |
| npm top 300 | 1 | — | ×0.9947, none worse by 5%, statuses unchanged |

quench-runtime lib: 788/788 on the candidate, and again on the merge into v2-gc.

Single workloads still swing by up to ±10% between rounds; in this round earley-boyer
was ×0.914 and hash-map ×0.900. Each suite geomean holds.

## Method

- **Compile cache.** `quench-node` caches compiled library modules in `~/.cache/quench`,
  keyed by the executable's path, size and modification time. A reused base binary had a
  warm cache while each rebuilt candidate started cold. Round 3 (`round3-warm-cache-*`)
  therefore showed npm ×1.021, an artifact. Round 1 built both binaries in one script and
  is fair; with the cache off, round 4 (this table) is fair too.
- **Instruction counts.** Callgrind on symbol builds (strip off) located the round 1 speed
  losses:
  - raytrace widened a 1-value arena range per object and regrew it.
  - Values were converted to narrow form twice per store.
  - Literals and planned construction packed twice.

  After the fixes, raytrace runs +5.6% instructions and lazy-collections +1.0%.
- **Reverted change, effect unknown.** A 4-value arena minimum, which removes raytrace's
  regrowth, was reverted after round 2 showed npm ×1.018. Round 2 had the same warm-cache
  bias as round 3, so that figure does not show the change was harmful. It is queued to be
  measured again with the cache off.
