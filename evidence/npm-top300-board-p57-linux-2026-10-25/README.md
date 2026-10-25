# npm top-300 board p57 (v2-npm 379619f410)

Host: x86_64 Linux VM, 4 vCPUs (Intel Xeon @ 2.10 GHz), 15 GiB RAM. Quench is a
PGO `production` build (sha256 `a8116129…41c8d`). Oracles: Node v26.12.0 and Bun
1.4.2. Each mode ran 4 rounds; round 0 is discarded as warm-up, and the medians
come from rounds 1–3 (`p57{n,j}{1,2,3}.json.gz`). `n` means no JIT; `j` means
`--jit` with `QUENCH_WASM_JIT_SELECTIVE=1`. All runs use
`QUENCH_STARTUP_SNAPSHOT=on`.

## Result

| mode | passing in every run | fastest wall | lowest peak RSS |
|---|---|---|---|
| no JIT | 299 / 300 | 283 | 291 |
| JIT | 299 / 300 | 281 | 289 |

The 300th scenario is `@rollup/rollup-linux-x64-musl`, a musl-linked native
addon on a glibc host. Node fails it too (`oracle-failed`).

## Losing rows by cause (`losing-rows.txt`, `classify.py`)

- **Bun threads.** Quench's CPU time is at or below Bun's, but Bun's wall time is
  lower because it spreads work across threads. Examples: typescript-eslint
  (no JIT) at 716 ms wall and 640 ms CPU, against Bun at 621 ms wall and 806 ms
  CPU; also playwright, schema-utils, ajv-formats, @babel/core and vitest.
  Collection is about 11% of typescript-eslint's instructions. It runs on the
  mutator thread because parallel marking is off while record families are
  live (GC lane).
- **Declaration scenarios.** These run the TypeScript checker as a native child
  process. The rows are wall/RSS ties within 7%, set by the child, which is the
  same binary under every runtime.
- **Built-ins (`jiti`, `undici`).** Bun runs neither package's code. It serves
  `undici` from a built-in module, and jiti chooses its native TypeScript loader
  when `"Bun" in globalThis` (`tryNative`). Under Node 26 and Quench, both run
  the npm package's own JavaScript; Quench beats Node on both (jiti 126 vs
  211 ms, undici 80 vs 127 ms, no JIT). Matching Bun would take
  package-identity behaviour, which the repository rules forbid.
- **Single-thread.**
  - color-convert (1.36x no JIT, 1.53x JIT) is compute-bound: 2,176 conversion
    cases with `Math.pow` and array callbacks, against JSC's JIT.
  - node-fetch is 1.06x.
  - Under JIT, axios (1.21x) and playwright (1.30x) also lose.

## `--jit` against no JIT, same binary

From the p57 binary, 9 interleaved runs:

| scenario | wall ratio (JIT / no JIT) | extra faults |
|---|---|---|
| semver | 1.15 | +1.2K |
| node-fetch | 1.10 | +1.4K |
| typescript-eslint | 1.08 | +3.0K |
| schema-utils | 1.07 | +3.7K |
| ajv-formats | 1.06 | +1.4K |
| color-convert | 1.05 | +1.4K |
| axios | 1.02 | +1.5K |
| playwright | 1.02 | +2.0K |

JIT runs have about 2 MB less RSS. Dropping the two unpaced native-compilation
page releases (plain release builds, 7 runs) removes about 1.3K faults, but
schema-utils stays +14% under JIT. Most of the cost is compilation and tier
admission, which the JIT lane owns.
