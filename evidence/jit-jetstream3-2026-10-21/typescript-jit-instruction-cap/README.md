# TypeScript JIT instruction-cap experiment

The current JavaScript JIT rejects candidates over 256 bytecode instructions.
The TypeScript diagnostic had 117 rows rejected by that cap, so a bounded
increase to 512 was screened against the existing 256-cap production binary.

The one-round diagnostic showed two additional functions compiling, including
`canHaveJSDoc` and `canHaveSymbol`; native entries rose from about 3.93M to
4.32M. It also emitted about 188 KiB of code. The JetStream score remained
0.05 and peak RSS remained about 567 MiB.

Three-round JetStream 3 results:

| Cap | Quench score | Median max RSS | Valid rounds |
| ---: | ---: | ---: | ---: |
| 256 | 0.08 | 564,088,832 bytes | 3/3 |
| 512 | 0.07 | 565,432,320 bytes | 3/3 |

The screens were sequential, and peer scores shifted, but the larger cap did
not produce a Quench score improvement and slightly increased RSS. The cap
increase was reverted. More native entries alone did not improve this fixture.

The reports retain the executable hashes and provenance:

- `cap256-score-3round.json`
- `cap512-diagnostic-1round.json`
- `cap512-score-3round.json`
