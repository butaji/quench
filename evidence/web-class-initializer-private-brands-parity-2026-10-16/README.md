# Private brands follow initialization; static blocks retain their context

Frozen source f17dbe4c068bb350e2a400b65e5af1746e594030 passes 381 serial Node v26.12.0 API comparisons, 667 runtime tests, workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes match before and after API verification. These results qualify this exact source and exclude later changes.

Node's class prototypes have no private brands merely from declarations. Static data fields remain absent until initialized, while static private methods/accessors precede field execution. The retained old probe incorrectly reports prototype membership and early static-data membership true. Corrected comparisons cover initialized receivers, field order, getter/setter pairs, stamping the class's own prototype, duplicate stamping and old values. Generic/specialized execution and binary round-trip verify these transitions.

Static blocks now retain their actual super home and execute in strict mode. Node comparisons cover inherited getters/setters with the actual class receiver, arrows, invalid ordinary-function super, undefined new.target, strict unresolved/readonly assignments and separate var scopes. The shared class-initializer context also prohibits lexical arguments in fields, static blocks and their arrows, while ordinary nested functions own arguments. The flag's existing serialized representation is retained in format 93.

An initial host build fails after erroneous deletion of an active incremental cache; it has no verification verdict. A later runtime run passes 666 cases and fails the existing auto-accessor regression: the hidden backing field depended on a prototype declaration brand for home discovery. Backing identities now retain their explicit lexical declaring-home binding, including source-invisible public instance auto-accessors. The complete 667-test retry passes. Raw failures remain distinct from successful checks; the early host retry predates the static-block edits and is not their proof.

All 32 matched instruction captures are valid with unchanged identities against 03ea582d2. The candidate uses the iteration profile (opt-level 1, 256 codegen units, no LTO) with incremental compilation disabled after cache/disk failures. Earlier snapshots used incremental compilation, so comparison includes build-configuration differences; the lower deltas cannot be attributed to this semantic repair. These integrated frozen-executable deltas do not establish production throughput/RSS. No authored optimization, total Node parity or web win is claimed. Ordinary NUL string-key visibility, broader private/stack/eval behavior and the wider Node backlog remain open.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
| crypto | +10.3908% | FAIL |
| deltablue | +12.9937% | FAIL |
| earley-boyer | +16.5963% | FAIL |
| navier-stokes | -6.5016% | pass |
| raytrace | +27.3743% | FAIL |
| regexp | -3.6836% | pass |
| richards | +24.1241% | FAIL |
| splay | +25.9162% | FAIL |
