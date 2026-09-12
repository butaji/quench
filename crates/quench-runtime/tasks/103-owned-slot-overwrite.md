# 103 — Ownership-aware environment and property slot overwrite

Status: complete

[[100-immediate-slot-overwrite]] proved that generic `Value` assignment spends material
time dispatching `Drop` for old immediate words that own no resource. Apply the same
single `Value::overwrite` operation to the remaining canonical owned slots:

- existing lexical/environment bindings, including resolved and cached writes;
- existing hidden-class property slots, including monomorphic IC hits;
- `PropertyStorage::insert` replacement, whose unused returned old value previously
  forced an immediate drop at the call boundary.

This is one representation of ownership semantics, reused across register, local,
environment, dense-element, and shaped-property storage. New property insertion still
pushes ownership normally; heap-tagged replacement still releases exactly one Rc.

Acceptance: all release tests and full smoke pass, then an exact alternating full-suite
A/B against the pre-change binary clears aggregate and component floors. Revert and
record the rejection otherwise.

## Result: accepted

All canonical environment and shaped-property replacements now use
`Value::overwrite`; new slots still take ownership normally. A dedicated ownership
test confirms that replacing heap strings in both storage kinds releases exactly one
Rc. All 43 release tests pass, and `reports/owned-slot-overwrite-smoke.jsonl` passes all
eight suites. The executable shrinks from 2,990,896 to 2,990,704 bytes.

The exact alternating six-repetition comparison in
`reports/owned-slot-overwrite-ab-6/comparison.txt` improves aggregate score from
772.048 to 774.371 (+0.30%). Crypto improves 2.97%, Earley-Boyer 2.49%, DeltaBlue
0.36%, Richards 0.12%, and Navier-Stokes 0.45%; RayTrace (-0.54%), RegExp (-2.64%),
and Splay (-0.68%) remain inside the component floor. The change is retained.
