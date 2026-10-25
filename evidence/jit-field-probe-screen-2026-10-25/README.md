# GeneralDispatch GetField probe screens

These short JetStream 3 screens evaluate a native-region helper for cached
ordinary-data `GetField` reads. Every candidate preserved output in 3/3 rounds
for the selected fixture. They are separate three-round runs, not paired
qualification measurements; the control binaries predate a documentation-only
commit, and host/peer medians vary between runs. Treat the score differences as
directional.

## Results

| Variant | Fixture | Quench Score | Max RSS | Node Score | Bun Score | Output |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Control | web-ssr | 14.69 | 117.2 MiB | 104.98 | 87.52 | valid 3/3 |
| Two-call probe | web-ssr | 17.21 | 118.6 MiB | 108.65 | 91.53 | valid 3/3 |
| Direct `Vm::step` on miss | web-ssr | 13.79 | 119.0 MiB | 97.32 | 96.53 | valid 3/3 |
| Batched callout on miss | web-ssr | 14.49 | 119.2 MiB | 82.45 | 83.92 | valid 3/3 |
| Control | hash-map | 7.52 | 43.2 MiB | 166.96 | 244.00 | valid 3/3 |
| Two-call probe | hash-map | 6.78 | 43.3 MiB | 162.28 | 185.59 | valid 3/3 |
| Direct `Vm::step` on miss | hash-map | 3.73 | 43.2 MiB | 161.49 | 208.48 | valid 3/3 |
| Batched callout on miss | hash-map | 7.54 | 43.1 MiB | 168.90 | 163.79 | valid 3/3 |

The direct one-step fallback lost fallback batching and regressed both Quench
fixtures, especially hash-map. Delegating misses to the existing batched
callout restored hash-map to the control range but did not improve either
score, while Web-SSR RSS stayed about 2 MiB above control. The original
two-call helper was mixed: a higher Web-SSR score and lower hash-map score,
with modestly higher Web-SSR RSS. None is a robust score/RSS win, so the code
prototype was removed.

## Reproduction

All runs used the pinned JetStream 3 corpus and production `quench-bench` with
JIT enabled, JavaScript GeneralDispatch enabled, three rounds, Node v26.12.0,
and Bun 1.4.3. The saved control binary is `/tmp/quench-node-field-probe-baseline`
(SHA-256 `22186f5e810540bdb47499696596e605917dad40283458bbd663fbf1618a3894`).
The two-call helper binary is `/tmp/quench-node-field-probe-candidate` (SHA-256
`4653d40db18bbae21e883041703585b4f427c38d9d5c7c8e5fdc0904a1c3dea7`), the
direct-step miss binary is `/tmp/quench-node-field-step` (SHA-256
`e045d632530816bc487fcf749ce9cd0c1666a586fc3da23856d61ce8637825ff`), and
the batched-callout miss binary is `/tmp/quench-node-field-batched` (SHA-256
`0ec1ec8538692f93e3dda8d4236c87db193a8c78ffad6b5c7ead3944a0dff5fa`).

Raw reports are `baseline.json`, `web-ssr-baseline.json`, `treatment.json`,
`single-call-treatment.json`, and `batched-treatment.json`. These data do not
qualify the JIT goal and do not establish a Copy-and-Patch compile-cost case.
