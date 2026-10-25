# Current hot-four JIT coverage diagnostic

This is one instrumented JetStream 3 round per fixture. All four produced
valid, output-matching results. `--jit-diagnostics` adds overhead; scores are
not qualification evidence. The report was generated with the saved control
binary `/tmp/quench-node-field-probe-baseline`, SHA-256
`22186f5e810540bdb47499696596e605917dad40283458bbd663fbf1618a3894`. Its
runtime source matches `v2-jit` `5f9b727b3` except for documentation changes.
The report was produced with the then-current production `quench-bench`, Node
v26.12.0, Bun 1.4.3, and GeneralDispatch enabled.

| Fixture | Quench score | Quench wall time | RSS | Native JS entries | OSR runs | Summed JS compile time |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| crypto | 158.04 | 3.07 s | 32.5 MiB | 2,477 | 8,157,111 | 30.10 ms |
| delta-blue | 48.13 | 10.75 s | 34.1 MiB | 392,771 | 0 | 36.65 ms |
| navier-stokes | 240.05 | 0.96 s | 34.1 MiB | 501 | 31,562 | 35.44 ms |
| richards | 42.58 | 13.37 s | 34.1 MiB | 22,160 | 0 | 8.59 ms |

Entry and region-run counts are not exclusive execution time. In Richards,
`Benchmark.runIteration` had 120 entries, no native entries, 5,999 general
dispatch work units against 107,200 required, and 13.16 s of interpreter
inclusive time. Its 9-instruction body has this loop:

```text
0000 LoadConst result=6 constant=0
0001 JumpBinaryFalse operator=4 b=r6 c=constant:30 target=6
0002 LoadCapture result=3 capture=0:0
0003 Call result=4 b=3 c=0 args=4+0 direct-eval=false parameter-eval=false
0004 IncDec result=6 b=6 flag=Some(false)
0005 Jump target=1
0006 LoadConst result=1 constant=10
0007 Return a=1
```

The OSR diagnostic reports one promotion-only site and a `LoadConst` analysis
rejection, despite no `LoadConst` in the reported loop range (PCs 1–5). This
needs a focused reproduction before editing OSR admission. The loop also calls
the captured `runRichards` function 50 times per iteration, so compiling its
control path alone cannot remove that callee's work.

The coverage suggests separate work: Crypto already has millions of OSR
runs but little native JS function coverage; NavierStokes reaches OSR but few
function entries; Richards spends almost all measured guest time under an
interpreted `runIteration`; DeltaBlue compiles many native entries but remains
slow. Compilation totals are small relative to fixture wall time in this
diagnostic, so this does not support starting a Copy-and-Patch backend to
address the observed score gap. Raw counters and commands are in `report.json`.

Reproduce:

```sh
target/production/quench-bench --fixtures crypto,delta-blue,navier-stokes,richards \
  --suite jetstream3 --jit --jit-diagnostics --jit-js-general-dispatch \
  --runs 1 --timeout-ms 600000 \
  --quench /tmp/quench-node-field-probe-baseline \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun target/tools/bun/bun-linux-x64/bun --qjs /bin/true \
  --out evidence/jit-coverage-current-diagnostic-2026-10-25/report.json
```
