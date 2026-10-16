# Executing operations and syntax-owned source locations

Frozen source 8e3dd52762121f7ee9e10a1ce88088d89d32a6db passes 362/362 serial API comparisons against official stable Node v26.12.0 (exit zero, 228.66 seconds), 653 Core tests, workspace/all-target checking, seven focused comparisons, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and API test hashes remain unchanged through the full run. The base e2e182a10, source patch and extra fixture hashes were reconciled before committing identical code. Subsequent fused-field/bytecode-format-91 changes are excluded.

Frames distinguish the executing guest-capable instruction from a saved continuation. Source capture projects the correct instruction without changing the GC cursor or lexical-binding owner. Call-state updates cover general and materialized lane calls; restored activations retain their continuation contract. A missing general-call update was caught by eval-root comparisons: file-entry calls matched, eval roots did not. The failure and temporary trace are retained; the common call path is fixed and temporary instrumentation removed before qualification.

OXC expression/comment spans identify real argument parentheses for computed, optional, private, parenthesized and function-valued calls. Comments containing parentheses are excluded by their parser-owned spans. Static member calls retain method-name positions. Reads and writes receive distinct positions for getters, setters, assignment, compound assignment and updates. Source lines recognize CR, LF, CRLF and Unicode separators; columns count UTF16 units. Oracle matrices cover these cases plus constructors, this(), returned functions and astral characters.

This candidate also includes shared native Buffer allocation/pooling. Both new Buffer oracle comparisons and their added runtime tests pass. Results are integrated measurements, not isolated attribution.

All 32 fresh strict matched Callgrind captures are valid against frozen 03ea582d2 with unchanged identities. These instruction checks are separate from production throughput/RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.4934% | pass |
| deltablue | -34.3873% | pass |
| earley-boyer | +1.7498% | FAIL |
| navier-stokes | -0.9642% | pass |
| raytrace | -11.8098% | pass |
| regexp | -6.0408% | pass |
| richards | -31.2662% | pass |
| splay | +0.6260% | pass |

Startup increases by 34,855,349–244,256,596 instructions. No authored optimization, total Node parity or web benchmark win is claimed.

The retained fused-field probe shows this.a.b still attributes its first getter to b's source position. Source metadata for individual fused components is subsequent work. Full eval/sourceURL, source-map, Node host-stack, internal-key visibility and broader API conformance remain open. Missing frames and source locations are never fabricated.
