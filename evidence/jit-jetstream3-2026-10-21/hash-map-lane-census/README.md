# Hash-map lane census and direct-call screen

The opt-in diagnostic now counts fast-lane function activations separately from
ordinary frame entries. This fixes a visibility gap: `get`, `_getEntry` and
`put` executed in the lane but previously had no JIT entry rows. The diagnostic
round reports 54 million `get` and `_getEntry` lane entries, 10.8 million
`put` lane entries, and 64.8 million native `computeHashCode` entries. `equals`
entered native code 53,999,969 times. This confirms substantial native helper
coverage and does not by itself demonstrate a guard redundancy or optimization
opportunity.

A follow-up attempted a direct lane-side call to the already-compiled numeric
callee. It used `js_jit_compiled_call`, retained its number guards and ordinary
fallback, and applied only to non-terminal same-program calls. The change was
removed after a matched three-round screen:

| Variant | Median Score | Median peak RSS | Per-round Scores |
| --- | ---: | ---: | --- |
| Control | 3.22 | 74.01 MiB | 3.71, 3.16, 3.22 |
| Direct lane call | 2.87 | 78.28 MiB | 2.19, 3.90, 2.87 |

Both reports were valid in all three rounds and output-equivalent. The direct
call lost 10.9% in median score and used 4.27 MiB more peak RSS, so it is not
retained. The broader JIT limitation remains: faster hash-map lookup does not
close the JetStream score gap to Node or Bun.

Reports:

- `diagnostic-1round.json`: lane and native-entry census, diagnostic mode.
- `control-3round.json`: existing call path, diagnostics disabled.
- `candidate-3round.json`: direct lane-side JIT call, diagnostics disabled.
- `lane-jit-3round.json`: initial unpaired candidate screen; not used to accept
  the change because its score was noisy against earlier runs.

## Direct Number equality screen

The numeric lookup path was also changed temporarily to use an exact bytecode
match for the `equals` Number path (`typeof` equality, object/function case
checks, then `a == b`). With both arguments guarded as Numbers, that path has no
coercions or effects and is equivalent to an IEEE `f64` comparison. The
diagnostic confirmed 53,999,999 direct comparisons and removed the repeated
compiled `equals` entry calls.

The same-binary three-round screen did not show a score win:

| Variant | Median Score | Median peak RSS | Per-round Scores |
| --- | ---: | ---: | --- |
| Equality call enabled | 4.57 | 74.01 MiB | 2.91, 4.57, 5.77 |
| Direct Number equality | 4.09 | 73.58 MiB | 5.14, 4.09, 3.74 |

Output matched for all rounds. RSS fell by 0.46 MiB, but Score fell by 10.5%
and only one of three adjacent score pairs favored the direct comparison. The
specialization was removed. The first unpaired three-round candidate screen
scored 4.33, but its variance and the same-binary result do not support keeping
it.

- `numeric-equality-confirmed-diagnostic.json`: confirms direct path coverage.
- `numeric-equality-off-3round.json`: same-binary existing equality call.
- `numeric-equality-on-3round.json`: same-binary direct Number equality.
- `numeric-equality-3round.json`: initial unpaired candidate screen.
