# Private data belongs to its declaring home and receiver

Frozen source 752c74a71e38095708bf55f08e6ad56579c1dab6 passes all 378 serial stable Node v26.12.0 API comparisons (267.64 seconds), 663 runtime tests (46.04 seconds), workspace/all-target checking, five focused private-related comparisons, 43 framework fixtures and nine no-JIT web correctness targets. Source/oracle/CLI/runner/test identities remain fixed through the API run; source-file hashes reconcile to the commit. Later lexical private-binding changes, including format 93, are excluded from this format-92 proof.

The original repeated-class probe reads [2,2] where Node reads [1,2]. A receiver now owns one traced entry for each actual (declaring home, logical name), containing both its brand and optional data. Values are no longer public string properties sharing one logical atom. An atomic private-data definition checks duplicates before mutation, preserves the old value after a failed second stamp and ignores public extensibility and Proxy traps. Private method brands likewise do not consult public extensibility. The new operation and its operands are serialized in bytecode format 92; throwing brand operations declare that effect.

The full guest comparison covers ordinary objects, arrays, functions, null prototypes, frozen/sealed/nonextensible targets, Proxy/revoked Proxy, duplicate error text and old values, frozen methods/accessors, static fields and cloning. The first storage repair leaves nine rows differing only in direct eval. Eval-capable methods now register their actual synthetic home capture in dynamic-name metadata, so eval does not infer private context from receiver prototypes. The complete stated matrix passes. Root tests retain two private values while the receiver lives and release both after dropping it; both execution modes and original/decoded execution pass.

A separate mixed private-eval probe remains failing: instance-method eval of static names, ordinary nested-function private eval and static-method eval of instance names differ from Node. Two same-home controls pass. This checkpoint does not claim complete lexical-private environments, ordinary NUL string-key namespaces or all class behavior. Those raw failures remain separate from the passing matrix.

All 32 strict matched instruction captures against 03ea582d2 are valid with frozen identities unchanged, but only two of eight 1% guards pass. These integrated deltas do not isolate attribution and are not throughput, RSS or benchmark dominance. A later private-binding type check runs alongside part of collection; wall times are not compared and the executables are independently frozen.

| Fixture | Fixed-work delta | 1% guard |
| -- | -- | -- |
| crypto | +14.8614% | FAIL |
| deltablue | +19.3071% | FAIL |
| earley-boyer | +24.1370% | FAIL |
| navier-stokes | -1.4287% | pass |
| raytrace | +33.3699% | FAIL |
| regexp | +0.1599% | pass |
| richards | +33.3264% | FAIL |
| splay | +36.4313% | FAIL |

Startup increases by 801,743,277–1,481,495,320 instructions. Six guard failures, named-lookup hook placement, three prototype warm-up differences, the ordinary NUL-key failure, broader private eval/source metadata and the full Node audit remain open. No authored performance optimization, total parity or winning web throughput result is claimed.
