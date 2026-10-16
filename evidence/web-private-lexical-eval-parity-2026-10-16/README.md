# Lexical private eval environments

Frozen source 6ded4eada9ecd399601ad59078f57251e5549b85 passes 379 serial Node v26.12.0 API comparisons, 665 runtime tests, workspace/all-target checks, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test hashes match before and after API verification. These results qualify this exact source, excluding later merges or repairs.

Private names now retain lexical bindings to each actual declaring home, separately from the method's super home. Mixed static/instance names, escaped ordinary closures, nearest class shadowing and outer private names are covered by the Node comparison. Generic/specialized execution and bytecode round-trip cover the added binding kind in format 93. Ordinary-function eval uses OXC's function grammar inside the lexical private-name parsing context; invalid super access, unknown private names and top-level return remain errors.

A retained malformed-eval probe previously returns 9 where Node throws SyntaxError. Parsed eval bodies must now match their generated source span, preventing user text from replacing the context wrapper's extracted body. The corrected Node comparison passes. Earlier type-check failures and unbounded probe outputs remain diagnostic records, not successful qualifications.

Separate probes still differ for super inside static-block eval, prototype private membership and static data membership before initialization. Ordinary NUL string-key visibility, broader private environment/brand behavior and the full Node backlog remain open. This is not total Node parity or a web throughput win. No authored performance optimization is claimed.

All 32 matched instruction captures are valid with unchanged identities against 03ea582d2. These integrated deltas do not isolate attribution or establish production throughput/RSS.

| Fixture | Instruction delta | 1% guard |
| --- | ---: | --- |
| crypto | +14.6888% | FAIL |
| deltablue | +18.4137% | FAIL |
| earley-boyer | +24.0519% | FAIL |
| navier-stokes | -1.4287% | pass |
| raytrace | +33.3557% | FAIL |
| regexp | -0.0359% | pass |
| richards | +33.2937% | FAIL |
| splay | +36.4230% | FAIL |
