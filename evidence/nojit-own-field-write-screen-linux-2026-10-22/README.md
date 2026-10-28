# No-JIT own-field write screen

This screen evaluates the conservative frameless `SetFieldStrict` path on
JetStream 3 Acorn WTB and Babylon WTB. The path accepts only an existing
writable own data slot on a plain shape-backed ordinary object. Other cases
continue through the lane. The experiment source tree was dirty; the report
records source revision `dfd2512cc079a1ed7b27846ab7f8d37f3d95a849` and the
binary hashes below.

Linux x86_64, four CPU quota, 16 GiB cgroup limit. Each binary was measured in
three rounds per fixture with the stock runner. Engine order was randomized
within each report, but all baseline rounds preceded all candidate rounds, so
the across-build scores are not a strict interleaved comparison. Both reports
have valid JetStream output and equal Quench output; this is a focused screen,
not a suite qualification.

| Fixture | Build | Quench median Score | Quench median max RSS | Bun median Score | Node `--jitless` median Score |
| -- | -- | --: | --: | --: | --: |
| Acorn WTB | baseline (`9b0b1e96…`) | 2.03 | 51.4 MiB | 2.14 | 3.11 |
| Acorn WTB | candidate (`d610f071…`) | 2.06 | 49.8 MiB | 2.53 | 3.40 |
| Babylon WTB | baseline (`9b0b1e96…`) | 2.72 | 48.0 MiB | 4.27 | 4.43 |
| Babylon WTB | candidate (`d610f071…`) | 2.70 | 46.5 MiB | 4.31 | 3.80 |

Candidate Quench scores moved +1.5% on Acorn and −0.7% on Babylon, within the
observed round spread. Candidate RSS was about 3% lower on both fixtures, but
separate build blocks and the lack of an expected allocation reduction make
that delta inconclusive. The change is therefore reverted; no performance win
is claimed. The full runtime lib test harness did not execute: both attempts
were SIGKILLed during rustc compilation at the container memory ceiling.
`cargo check --locked --profile iteration -p quench-runtime --lib --tests`
passed before the screen.

Reproduction:

```sh
target/production/quench-bench --fixtures acorn-wtb,babylon-wtb \
  --suite jetstream3 --runs 3 --timeout-ms 600000 \
  --quench <quench-node-binary> \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true --out <report.json>
```

The raw reports are `own-field-write-baseline.json` and
`own-field-write-candidate.json`.
