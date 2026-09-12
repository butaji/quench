# 374 — Native inherited-property cache projection experiment

Status: complete

Test whether Task 373's general direct `GetStatic` stencil should consume the existing
guarded prototype-chain cache as well as its own-property cache. The experiment added a
`repr(C)` projection containing receiver shape, immutable guard-chain pointer/length, and
holder slot. The cooked leaf walked object identities and shapes directly, loaded the
holder slot on a complete match, and used the canonical slow connector on any mismatch.
Focused tests covered an inherited hit and rejection after prototype shape mutation.

The projection was semantically correct and all 138 release tests plus the eight-suite
smoke passed, but reach was negligible. At matched 100 ms residual probes, DeltaBlue's
Rust inherited-cache counts and generic block entries were exactly unchanged. RayTrace
removed only about 8,000 generic entries. Earley-Boyer had no inherited hits. The reason is
structural: nearly all important inherited accesses occur in blocks that also contain an
unsupported `LoadName`, `Call`, or other effect, so Task 373's all-or-nothing selector never
reaches the new leaf.

The three-pair, 200 ms complete-suite A/B measured 2431.38 -> 2430.06 (-0.05%). The native
projection and expanded per-site cache were therefore removed. The only retained change is
`stencil-aot/object_layout.rs`, which replaces duplicated object-layout magic numbers with
one named ABI constant set. The rebuilt Mach-O `__TEXT,__text` hash remains byte-identical
to the accepted Task 373 executable (`142fed8d5d35a676aa1c28e74f4909aef5400c1262d45eee056d93a12b84ac30`).

This experiment sharpens Task 368: prototype-chain native code is not worth revisiting
until the enclosing name/call block can stay native as one coarser morphism.

Reports:

- `reports/task374-native-inherited-property-smoke.jsonl`
- `reports/task374-native-inherited-property-ab-200ms-3/`
- `reports/task374-native-inherited-property-residual/`
- `reports/task374-baseline-prototype-residual/`

