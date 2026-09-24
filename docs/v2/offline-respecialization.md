# Offline profile-guided re-specialization

rqj can iterate Futamura P1 only when the second specialization preserves the
semantics of the first residual program. A profile is evidence about one run;
it is not a proof about every future execution. This distinction determines
which facts may be consumed without adding runtime guards.

## Sound fact classes

| Profile fact | Guard-free use | Soundness requirement |
| --- | --- | --- |
| Opcode/window frequency | Select or order semantics-preserving rewrite recipes | Every selected recipe must preserve both source instructions and pass the effect-union check |
| Function/call frequency | Choose among equivalent interpreter layouts | Every layout must implement the complete opcode semantics |
| Branch bias | Lay out blocks or rank fusion candidates | Both successors remain reachable; no branch is deleted |
| Cache hit/miss frequency | Size or order cache tiers | The ordinary shape/key check and miss path remain |
| Observed value/type/shape | None without an existing semantic guard | Observation alone cannot justify substitution, unboxing, branch deletion, or direct property access |

The last row is the critical limit. A site that saw one shape in a training
run may see another shape on the next call. Promoting the observed shape to an
unconditional static fact changes the program. Retaining a shape check and
fallback is correct, but then the mechanism is cache/layout tuning rather than
guard-free constant promotion.

## Two-phase pipeline

1. Compile source with OXC and the normal generating extension.
2. Execute the residual under feature-gated aggregate/trace instrumentation.
3. Join events to stable residual function/PC identities. Function names,
   source filenames, and benchmark identities are not inputs.
4. Feed only sound classes from the table above into a second specialization.
5. Emit ordinary interpreted bytecode. The second execution has no counters
   and no checks beyond the semantic checks already required by that opcode.

The profile and residual must carry matching format and structural hashes. A
mismatch discards the hints rather than applying them to different code.

## Feasibility result

Tasks 164–166 instantiate the highest-frequency opportunity exposed by the
current Crypto profile: `Binary→Binary` executes roughly 379 million times.
They test three increasingly specialized second-pass residual layouts:

- generic four-operation data windows;
- an unrolled `Binary×4` window;
- the same unrolled window preserving the numeric integer fast path, globally
  and then only in the bytecode-derived numeric dispatch class.

All variants preserve semantics, contain no runtime profile guard, and are
selected from opcode/effect/control-flow data. The strongest form makes Crypto
slightly faster locally (1508 versus 1501) but raises median RSS by 147 KiB and
regresses Richards Score/RSS. The generic and unrolled forms are slower.
Consequently the measured second-pass opportunity does not pay for its extra
residual/handler layout under the cumulative gate.

Other hot profile-guided candidates have already been measured independently:
numeric operand-mode tables (task 153), binary pairs (141), local sinks
(131–150), and handler layout variants (139–146). They show the same pattern:
small local Score gains are outweighed by touched-page RSS or earlier-workload
regressions.

## Decision

rqj retains the profile data and offline candidate miner as inputs for future
recipes, but does not add a production two-run pipeline now. Observed values
are never promoted as unconditional facts, and the sound layout-only spike
fails the current Score/RSS ratchet. A future recipe can reopen this decision
only with a semantics-preserving residual transformation that wins all
cumulative gates.
