# JetStream 3 `web-ssr` JIT baseline

This is an 11-round matched run from clean source commit `b245885ff1e7a9b4f2b75579b5db68fc542c9902`, using the pinned JetStream 3.0 corpus at
`06785cf861ac44855f168cbbe829278c2802e6de` on Linux x86_64. Every round had
valid scores and maximum RSS, and all three engines produced equal semantic
output. Node v26.10.0 and Bun 1.4.2 were verified with their JITs enabled.

Reports:

- [`web-ssr-jit-11round-clean.json`](web-ssr-jit-11round-clean.json)
- [`web-ssr-jit-11round-clean-analysis.json`](web-ssr-jit-11round-clean-analysis.json)
- [`web-ssr-jit-diagnostic-final.json`](web-ssr-jit-diagnostic-final.json)

Median Score / maximum RSS:

| Engine | Score | Max RSS |
| --- | ---: | ---: |
| Quench JIT | 15.83 | 128.75 MiB |
| Node JIT | 117.36 | 215.33 MiB |
| Bun JIT | 119.97 | 232.08 MiB |

The paired 95% ratio intervals are Quench / comparator:

| Comparator | Score ratio | Max RSS ratio | Result |
| --- | ---: | ---: | --- |
| Bun | 0.1246 (0.1222–0.1418) | 0.5558 (0.5403–0.5601) | RSS lower; Score loses |
| Node | 0.1384 (0.1161–0.1473) | 0.5977 (0.5962–0.6073) | RSS lower; Score loses |

The paired intervals exclude ties. This fixture does not qualify because the
Score gate fails against both JIT peers. M4 was not available in this
environment, and this result does not cover the other JetStream fixtures.

The diagnostic round identifies where to extend the JavaScript tier. React's
`web-ssr/dist/bundle.min.js` functions `Ru` and `Eu` dominate inclusive
interpreter time across repeated eval programs. `Ru` has 831 bytecode
instructions, two parameters, and zero numeric entries; its current rejection
is the bytecode-size ceiling. `Eu` has 53 instructions, four parameters, zero
numeric entries, and is rejected by the activation-kind filter. Their source
lines are 1105 and 1103 in the minified bundle. Inclusive times overlap across
callers and callees and are only used to rank compiler work.

## Opcode profile

[`web-ssr-jit-opcodes-clean.json`](web-ssr-jit-opcodes-clean.json) is a
single diagnostic run from clean source commit `2697779eed4377f44dbf9d198548984d169840a9`,
with the opcode histogram enabled. It is profiling evidence only: diagnostics
change execution cost, and its score is not a qualification result. The profile
confirms the primary renderer costs are generic tagged-value work: `Ru` contains
831 instructions, including 47 calls, 49 conditional branches, 287 local
loads/stores, 18 indexed reads, array/object construction, and 9 throws. The
53-instruction `Eu` wrapper has 21 local loads and 5 stores, 3 capture loads,
and one call. Only one JS function entered native code in this sample
(`quickHash`, 61 entries); the dominant render functions stayed in the
interpreter. A general tagged-value JIT tier is required before these results
can move materially.
