# Canonical dynamic-Function source and scope

Frozen source 5acfdf424147f0f965fc76353d2894029beb896f includes shared prepared-policy merge b7723be8e and passes 377/377 serial stable Node v26.12.0 API comparisons (267.03 seconds), 661 runtime tests (46.50 seconds), workspace/all-target checking, 76 source-layout oracle rows, the earlier native-info controls, 43 framework fixtures and nine no-JIT web correctness targets. Source, oracle, CLI, runner and test executable identities remain fixed through the API run. Source-file hashes reconcile to the commit. Subsequent private-field storage/bytecode changes are excluded.

The retained 20-case negative control differs in every row: Function body trimming loses whitespace and the compiler wrapper places body expressions on the header line. A single kind-owned canonical renderer now supplies compilation and public toString. Whitespace and all JavaScript line separators survive; parameters, body captures, nested closures and eval origins match the 76-row oracle across four kinds. OXC validates the generated name and parameter boundary, then only the synthetic outer identifier is removed before semantic binding: anonymous is not a self binding and actual parameter/nested names remain intact.

The first core parameter-column expectation (24) fails on Node itself; the actual result is 25. V8 counts its hidden opening parenthesis in first-line parameter positions. The corrected control and compiler retain that wrapper directly, without a compensating source-column offset. The initial failed oracle and corrected success remain separate artifacts.

Pre-merge source passes 660 runtime tests and the 76-row comparison. A fresh sync consumes 0198edb75 shared prepared-policy code and changes the integration boundary. Both parents' test chunks survive; one moved glob test is deduplicated and one external blank line differs. No guest source/assertion is discarded. Merged workspace checks pass, then the restored source-layout repair is rebuilt and requalified with all 661 runtime tests and 377 APIs. Parent proof is not reused as integration proof. The preservation receipt counts column-zero chunks; the actual Rust runner count is authoritative.

All 32 matched instruction captures against 03ea582d2 are valid with frozen identities unchanged. Only two of eight 1% guards pass. These are integrated deltas, not isolated attribution, throughput, RSS or a benchmark win. A next-stage private-field type check/build runs alongside part of instruction collection; wall times are not compared and measurement uses independently frozen executables.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +14.8573% | FAIL |
| deltablue | +19.3171% | FAIL |
| earley-boyer | +24.1372% | FAIL |
| navier-stokes | -1.4290% | pass |
| raytrace | +33.3686% | FAIL |
| regexp | +0.1535% | pass |
| richards | +33.3242% | FAIL |
| splay | +36.4424% | FAIL |

Startup increases by 802,559,735–1,481,324,212 instructions. The named native-info hook's placement remains an instruction-cost finding; no authored performance optimization has begun. Three prototype feedback/warm-up differences, ordinary NUL-key visibility, repeated-class private-field identity, raw UTF16 and normalized-parameter source metadata, sourceURL/source maps, missing stack methods and the broader Node conformance audit remain open. Total Node parity and a winning web throughput result are not established.
