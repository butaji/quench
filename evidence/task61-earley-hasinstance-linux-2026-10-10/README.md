# Rejected EarleyBoyer built-in `@@hasInstance` call bypass

The scoped profile showed 17.19 million `instanceof` binary operations, with
16.65 million immediately preceded by `LoadCapture`. I tried bypassing
`call_value` when `@@hasInstance` resolved to Quench's exact native
`Function.prototype[@@hasInstance]`, calling the existing
`ordinary_has_instance` algorithm directly. Custom methods and proxy behavior
kept the normal path.

A direct Node v24.19.0 oracle covered ordinary and bound functions, custom and
non-callable `@@hasInstance`, an accessor override, proxy method/prototype
traps, primitives, and thrown outcomes. Baseline and candidate matched Node's
outcomes and observable traces and matched each other. Three error message
strings already differ from Node in the unchanged baseline; those are recorded
in `node-oracle-report.json`.

Three alternating production pairs on the pinned EarleyBoyer input all
matched output, but the candidate regressed in every pair:

- Median Score: 428 baseline, 407 candidate; paired median delta −23.
- Median maximum RSS: 41,644,032 bytes baseline, 47,693,824 candidate; paired median delta +6,152,192 bytes.
- Score was lower and RSS higher in all three pairs.

This was a three-pair rejection screen, not a qualification interval. I removed
the source change. Binary hashes are `e4653782…` baseline and `f4203321…`
candidate; the exact fixture hash and ordered rows are in `screen-3.json`.
Raw stdout/stderr, the source patch, Node oracle, and online research notes are
included in this folder.
