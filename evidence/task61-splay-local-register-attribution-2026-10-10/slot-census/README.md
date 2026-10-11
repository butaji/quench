# Splay fixed-register parameter promotion

This record contains the counter census, fixed-work gates, compiler/runtime
safety checks, and oracle probe for promoting immutable simple parameters into
stable frame registers. It is a general compiler rule: production does not
inspect fixture identity or source text. The compiler metadata is the single
binding-location authority; captured, mutable, lexical, mapped-arguments,
with/eval-context, and other ineligible bindings retain their existing paths.

## Coverage census

The pinned counter binary is `target/pinned/b2600310…/quench-node` (SHA-256
`b2600310b6f404eeac016ac7944c28209600733a77be27db27efc5e6567e4a02`), built
from the dirty tree at `7dfe2e1f1` with `profile-aggregate,profile-memory`.
`QUENCH_DISABLE_LOCAL_PROMOTION=1` selects the same-tree baseline; leaving it
unset selects the candidate. Both modes contain the same local-operand
forwarding and unrelated source state. The fixed-work counters compare setup
plus one Splay `run()` with setup-only.

| One `run()` | No promotion | Parameter promotion | Change |
| --- | ---: | ---: | ---: |
| Physical dispatches | 181,454 | 140,752 | −40,702 (−22.43%) |
| `LoadLocalPlain` | 52,311 | 28,089 | −24,222 |
| `StoreLocalPlain` | 20,645 | 8,725 | −11,920 |
| `Move` | 11,280 | 6,720 | −4,560 |
| Plain-local operations | 72,956 | 36,814 | −36,142 (−49.54%) |

The promotion map accounts for every removed local operation. It removes
25,120 operations from `GeneratePayloadTree(depth, tag)` and 6,462 from
`SplayTree.splay_`'s `key` parameter. The remaining 36,814 local operations
are non-parameter bindings, led by mutable locals in `splay_` (34,812). The
earlier 82,957 count used a different working-tree state; the same-source
baseline here is 72,956 because local-operand forwarding is active in both
modes. The analyzer verifies global dispatch totals, local-op totals and the
promotion-map delta against the captured profiles.

## Fixed-work decision

The candidate and baseline are pinned production builds of the same dirty
source tree, with only the `QUENCH_DISABLE_LOCAL_PROMOTION` compile-time switch
differing. Baseline SHA-256 is
`c417d8c41a5e2a370f7054b11923a29a44ddb721e287279ba7d42cd88f9524a6`; candidate
SHA-256 is
`4ad2b512f363998cbe1e85b60ddc697fcadf23eabc666a68657ecfa0ee001b99`. The
runner is `quench-bench` SHA-256
`694afd730d95b5cb142bac1deaca3d7b43174ebd8613716b2517884d491cea8b`.

Splay completed 11/11 detector-clean pairs. Marginal instructions/run fell
8.198% and cycles/run fell 6.792% (paired-bootstrap 95% cycle interval
−7.308% to −6.261%). Median maximum RSS changed from 168,427,520 to
168,214,528 bytes (−0.126%). Against the Step 0 Node `--jitless` marginal
cycle target of 4,590,844, Splay moved from 9,650,696 cycles/run (2.10×) to
8,996,543 cycles/run (1.96×). Maximum RSS is 168.21 MB versus Bun no-JIT's
57.95 MB, a remaining 110.26 MB gap. This is progress, not a Task 61 win; the
stock-harness Score and RSS bars still need to be cleared.

The all-eight guard requested for this dispatch-count change passed. Each
fixture had 11 valid pairs, instructions stayed within the measured +0.14% A/A
band, and median maximum RSS stayed within the +0.5% budget. The paired results
were:

| Fixture | Clean cycle pairs | Instructions | Cycles | Max RSS |
| --- | ---: | ---: | ---: | ---: |
| Crypto | 0/11 | −0.583% | unavailable | −1.085% |
| DeltaBlue | 11/11 | −0.868% | −0.568% | −0.491% |
| EarleyBoyer | 7/11 | −4.179% | −2.252% | −0.197% |
| NavierStokes | 2/11 | −4.956% | unavailable | −0.080% |
| RayTrace | 11/11 | −2.772% | −2.212% | −0.620% |
| RegExp | 11/11 | −0.0001% | +0.535% (interval includes zero) | −1.309% |
| Richards | 11/11 | −2.562% | −1.781% | −0.704% |
| Splay | 11/11 | −8.198% | −6.792% | −0.126% |

Crypto and NavierStokes were classified by the approved instructions-only
fallback because host contention prevented the seven-clean-cycle minimum; the
candidate did not exceed the instruction A/A band on either fixture. The other
six fixtures had enough clean cycle samples and showed no detected regression.
The machine-readable guard decision is authoritative for pair counts,
intervals, hashes and classifications.

## Correctness checks

The isolated staged source was applied to a clean worktree at `7dfe2e1f1`. A
one-line generator profiler API compatibility fix, committed separately as
`47c6e7026`, was included in that test tree. The isolated source passed the
promotion eligibility, bytecode-validator, residual-roundtrip and
runtime-order/aliasing/GC unit tests, then the full runtime library suite
(544/544). The full candidate also passed Test262
arguments-object 263/263, eval-code 347/347, function-code 217/217 and
rest-parameters 11/11. Expression Stage 10 had 15 failures, the repository's
previously recorded compound-assignment failures. A Node oracle probe covering
parameter reads, missing arguments, ordering, mutation exclusions, mapped
arguments, nested closures, delete and direct eval matched stdout and stderr
byte-for-byte (exit 0).

See `promotion-coverage.json` and `analyze_local_slots.py` for the census;
`parameter-promotion-all-eight-11.json` and `parameter-promotion-guard.decision.json`
for the raw gate and decision; and `validation.json` for correctness details and
output hashes. `splay-promotion-fixed-work-11-initial.json` preserves the
superseded initial five-clean-pair screen.
