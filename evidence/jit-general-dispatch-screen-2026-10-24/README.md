# GeneralDispatch connection screen · 2026-10-24

Source revision: `b40a01742d88d986f9f01e75a26ad44cf54f2ceb` (clean for both runs). Linux x86_64, four-CPU quota, 16 GiB memory limit, JetStream 3 corpus `06785cf861ac44855f168cbbe829278c2802e6de`, Node v26.12.0, Bun 1.4.3. Production binaries were already built at this source revision.

This screen verifies the connection switch and establishes that the broad GeneralDispatch path is not active under ordinary `--jit`. The external `QUENCH_JS_JIT_GENERAL_DISPATCH=1` environment variable was intentionally ignored by `quench-bench`'s child environment, so the first run is a normal control (`jit_js_general_dispatch=false`). The second run uses `--jit-js-general-dispatch` (`true`). Both are valid and output-equal for all three rounds.

| Mode | Quench Score | Quench peak RSS | Bun Score / RSS | Node Score / RSS |
|---|---:|---:|---:|---:|
| `--jit` (general tier off) | 14.85 | 113.7 MiB | 56.12 / 223.0 MiB | 79.43 / 211.2 MiB |
| `--jit --jit-js-general-dispatch` | 13.54 | 115.2 MiB | 78.41 / 229.8 MiB | 80.02 / 201.6 MiB |

The reports are separate three-round runs, not an interleaved causal comparison. The peer medians moved substantially between runs, and three rounds are below the qualification minimum of eleven. This does not show a performance win and does not justify enabling GeneralDispatch by default. An earlier 11-round screen at source `2583c662a` is recorded in `evidence/jit-jetstream3-relations-screen-2026-10-24/`; its treatment remains diagnostic/non-qualifying and far below peer scores.

Raw reports:

- `web-ssr-control-3rounds.json`
- `web-ssr-treatment-3rounds.json`

Commands used `target/production/quench-bench`, the clean production `quench-node` binary at source revision above, the pinned Node/Bun paths, and `--qjs /bin/true` (JetStream 3 does not use QuickJS). Treatment command:

```sh
target/production/quench-bench --fixtures web-ssr --suite jetstream3 --jit \
  --jit-js-general-dispatch --runs 3 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun target/tools/bun/bun-linux-x64/bun --qjs /bin/true \
  --out evidence/jit-general-dispatch-screen-2026-10-24/web-ssr-treatment-3rounds.json
```

## Disabled-tier entry-gate patch screen

The current patch keeps the general-tier switch off in ordinary `--jit` and
removes a repeated per-bytecode tier-table lookup when that tier cannot be
active. The pre-change production binary SHA-256 is
`1a0302dd5c9db2fce3db5a472fd8465fb9f0fac98449af07a3ec4b005f620bf7`; the
post-change binary SHA-256 is
`33df1cedf3cadf9601ffa657fddd6a43113d6bc239e80883161872d4d7991108`.
The treatment source tree was dirty only for this patch. All four reports are
valid, 3/3 output-equal web-ssr rounds:

| Mode | Arm | Quench Score / RSS MiB | Bun Score | Node Score |
|---|---|---:|---:|---:|
| JIT | control | 15.86 / 116.4 | 77.62 | 90.92 |
| JIT | patch | 17.23 / 113.5 | 87.90 | 80.88 |
| no-JIT | control | 16.68 / 118.0 | 45.94 | 52.14 |
| no-JIT | patch | 14.42 / 114.9 | 49.47 | 58.00 |

These were separate runs, not interleaved. Peer scores shifted between runs,
so the differences do not establish a causal speedup or slowdown. Both Quench
modes remain far below the peers. The analyzer requires 11+ rounds; these
screens do not qualify the target. Use them only as a smoke check for output
parity and gross memory changes. The following raw reports retain the engine
hashes, order, process measurements, and per-round outputs:

- `entry-gate-jit-control-3rounds.json`
- `entry-gate-jit-treatment-3rounds.json`
- `entry-gate-nojit-control-3rounds.json`
- `entry-gate-nojit-treatment-3rounds.json`
