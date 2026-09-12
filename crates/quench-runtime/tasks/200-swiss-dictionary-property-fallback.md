# 200 — Swiss-table dictionary-property fallback

Status: planned

Profile-gated only: when an object genuinely leaves immutable shape mode, replace the
general string-keyed map with Swiss-table control bytes, short hash fragments, and
group-wise matching. Preserve ECMAScript enumeration order in a separate compact order
vector. Interned atoms and tagged integer keys remain the canonical key identities.

This does not compete with shape/slot ICs: it is the shared megamorphic/dictionary kernel
for objects with frequent dynamic insertion/deletion. SIMD group width, load factor,
growth factor, and tombstone threshold are named architecture policy constants.

Acceptance: profile evidence first proves dictionary probing material; insertion,
deletion, reinsertion, enumeration order, collisions, and adversarial keys pass; NEON or
scalar group probing is selected explicitly; full V8v7 A/B improves without weakening
HashDoS policy for externally controlled keys.

Primary sources: Abseil Swiss tables <https://abseil.io/about/design/swisstables> and
V8's SwissNameDictionary
<https://chromium.googlesource.com/v8/v8.git/+/refs/heads/12.0.78/src/objects/swiss-name-dictionary-inl.h>.

