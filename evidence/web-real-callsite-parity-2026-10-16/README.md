# Actual detailed source frames and call positions

Frozen source 0a6037fc7173ccd8b39b75eca28d9b03ece5753c passes 356/356 serial API comparisons against official stable Node v26.12.0 (exit zero, 223.73 seconds), 651 Core tests, workspace/all-target checking, 43 framework fixtures and nine no-JIT web correctness targets. The shared generator-pattern iterator-cleanup regression also passes as an isolated subset test. CLI, runner and test hashes remain unchanged through the full API run. The recorded base dbac9a481, source patch and fixture hashes were reconciled before committing identical source. Later operation-PC/token-position changes are excluded.

The public util.getCallSites function comes from unmodified configured Node util.js. Its native operation projects actual JavaScript activations independently of Error hooks/limits, skips the utility wrapper, omits native helper frames and returns null-prototype records with copied UTF16 names, source identities, filenames, positions and the column alias. Source identities derive from actual compiled units and are not fabricated from argv. Error CallSites and detailed capture share source projection; host filename mapping remains an explicit mutable boundary.

The oracle comparison covers mutable/accessor-fallback/UTF16 names, positive source identities and equality within a source, exact first-frame positions, descriptors/key order, Error-hook independence, validation and the original function's reflection/source. The implemented call-column comparison covers six identifier/member/computed/parenthesized/constructor forms. These selected passes do not establish complete stack conformance.

Initial failures are retained: a read-only helper could not call the mutable host filename mapper; the first public comparison failed because the configured-library binding bridge did not expose the new native operation. Explicit host-effect handling and the missing delegate repair both issues. After the shared lazy-library/transition-index merge, workspace checks pass, but the first harness build exhausts disk while LLVM writes bitcode. Its diagnostics are build/environment failures, not test results. Inactive previous-graph caches were removed; the retry and both focused comparisons pass before full qualification.

All 32 fresh strict matched Callgrind captures are valid against frozen 03ea582d2, with unchanged identities. Seven guards pass; Earley-Boyer remains above 1%. Integrated results include shared lazy library loading and transition-index changes and do not isolate attribution.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.3433% | pass |
| deltablue | -34.4414% | pass |
| earley-boyer | +1.5519% | FAIL |
| navier-stokes | -1.0409% | pass |
| raytrace | -11.9011% | pass |
| regexp | -6.1496% | pass |
| richards | -31.2524% | pass |
| splay | +0.6045% | pass |

Startup increases by 17,782,525–230,597,788 instructions. No production throughput/RSS, authored optimization, total Node parity or web benchmark win is claimed.

Retained new negative controls show optional/private/complex calls and comments before call parentheses still have incorrect positions. Getter/setter probes also reveal that source capture subtracts one from a PC which sometimes denotes the executing operation rather than a saved call continuation. Those repairs are subsequent work. Complete source-map/eval/sourceURL/host-stack support, internal-key visibility, allocation provenance and broader Node APIs remain open. Missing Node host frames are not fabricated.
