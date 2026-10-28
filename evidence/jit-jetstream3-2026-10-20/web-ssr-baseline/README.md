# Web-SSR baseline on the resumed cloud workspace

These one-round screens use the pinned JetStream 3 fixture, Node 26.10.0, and
Bun 1.4.2 on Linux x86_64 with four vCPUs. They are local screens, not
qualification evidence: each has one round, the reports mark the source dirty,
and the JIT and interpreter screens were run separately.

| Mode / runtime | Score | Peak RSS |
| --- | ---: | ---: |
| Quench JIT | 6.06 | 135.5 MiB |
| Quench interpreter | 5.77 | 133.1 MiB |
| Bun JIT | 81.45 | 227.8 MiB |
| Node JIT | 92.42 | 203.6 MiB |

The diagnostic run validates stock output but adds instrumentation. It measured
Quench at 2.94 Score / 137.5 MiB and recorded only one compiled function,
`quickHash` in the benchmark driver (61 native entries). The generated React
functions `Ru` and `Eu` remained interpreted: `Ru` was over the 256-bytecode
candidate cap at 831 instructions, while `Eu` contained capture and call
operations outside the current narrow compiler.

This evidence identifies Web-SSR as still far from the JIT peer score bars.
The native tier needs broader production-function coverage before this fixture
can win. Raw runner reports are in this directory.
