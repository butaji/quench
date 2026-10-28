# Direct entry for guarded Int32 identity functions

`Call` and `CallKnown` now enter an already-compiled Int32 identity helper
directly when its argument is an Int32. Fractions and non-number values still
take the ordinary VM frame path. The helper remains pure and the caller only
receives its return value after the existing Int32 guard succeeds.

The focused runtime test code type-checks with
`cargo check --profile iteration -p quench-runtime --tests`; compiling its test
binary exceeded 29 minutes in this cloud workspace and was stopped before
execution. The optimized `quench-node` build and CLI JIT smoke passed. The
stock `hash-map` fixture validated in both one-round runs below.

| Binary | Quench Score | Peak RSS | Quench wall time |
| --- | ---: | ---: | ---: |
| Parent `6eb065f` | 0.91 | 75.2 MiB | 432.29 s |
| Direct Int32 entry | 1.94 | 75.3 MiB | 277.44 s |

These are separate single rounds, not paired evidence. Bun and Node also varied
between runs, so the apparent 2.1x score and 1.6x wall-time improvement is
directional only. The change is not a JetStream win: Quench remains far behind
Bun (157.99) and Node (161.69) in the direct-entry screen. Run matched rounds
before making a performance claim. The JSON reports preserve exact commands,
engine hashes, validation, RSS and host provenance.
