# Eval executes parsed programs with original source positions

Frozen source bae787fce7e5638a50d7cb94809a942583460f6e passes 365/365 serial comparisons against stable Node v26.12.0 (exit zero, 229.56 seconds), 654 runtime tests, workspace/all-target checking, an expanded eval oracle matrix, seven earlier source/stack matrices, 43 framework fixtures and nine no-JIT web correctness targets. CLI, runner and test hashes remain unchanged through the full API run. The source patch and added fixture reconcile to this commit; its publication base differs from its compilation base only by evidence. Later source-excerpt and eval-origin edits are excluded.

The retained negative controls show valid eval string "for(;false;)" throws SyntaxError and hashbang Math.max(2,3) returns undefined where Node returns 3. Direct/indirect eval now execute the existing OXC Eval goal, and host evaluation uses the Script goal, preserving raw source and statement completion. More than 1,100 obsolete lines are removed: the manual text interpreter, expression-return wrappers, redundant grammar helpers, fallback error flag and wrapper-only binding state. The Function-constructor compiler retains unconditional parameter-boundary validation.

OXC owns regexp literal spans and flags; the original UTF16 pattern units remain authoritative, including lone surrogates and parentheses. Expanded oracle comparisons cover completion, strictness, syntax errors, hashbangs, caller bindings, arguments, new.target, original call positions, host Script expressions and regexp units. The original probes now show the correct values and eval columns 1/3:1 instead of wrapper column 22. Their complete raw stacks still expose the separately open filename/origin/host-frame gaps.

Initial borrowed-slice and obsolete-dead-code check failures are retained separately from successful checks. Completed compiler caches are removed as needed; binaries, SDKs and measurement evidence are preserved.

All 32 fresh strict matched captures are valid with unchanged frozen binary identities against 03ea582d2. Startup subtraction uses each binary's N=0 capture; regular-file outputs must complete the exact fixture count. These are integrated instruction measurements, not isolated attribution, throughput, RSS or benchmark dominance.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +0.5632% | pass |
| deltablue | -34.2913% | pass |
| earley-boyer | +1.5972% | FAIL |
| navier-stokes | -0.9791% | pass |
| raytrace | -11.9788% | pass |
| regexp | -6.1325% | pass |
| richards | -31.2700% | pass |
| splay | +0.9125% | pass |

Startup increases by 35,701,089–244,322,545 instructions. No authored optimization, total Node parity or web benchmark win is claimed. Complete eval-origin/sourceURL/source-map metadata, Function-constructor source positions, fatal excerpts, internal-key visibility and broader Node APIs remain open in this frozen snapshot.
