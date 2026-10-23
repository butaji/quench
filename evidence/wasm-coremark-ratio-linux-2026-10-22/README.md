# CoreMark ratio campaign, Linux x86_64, 2026-10-22

Goal (TOD-182): quench-node at least 1.2x every other runtime, no JIT and JIT.

* Host: cloud VM, Intel Xeon Processor @ 2.10GHz, 4 vCPU, 15 GB RAM, no
  hardware performance counters.
* Binary: static production `quench-node` at `v2-cloud-wasm` `61b0cce31`
  (= `v2-merged`), bolted with the standard training list.
* Script: `evidence/wasm-esm-runner-linux-2026-10-19/run-coremark-esm.sh`,
  11 interleaved rounds. Data: `coremark-esm8-11.csv`.

| Mode | quench-node | Best other | Median per-round ratio | Rounds ≥ 1.2x |
| -- | --: | --: | --: | -- |
| No JIT, Wasm ES module | 3,519.9 | Wasmi 2,981.1 | 1.217 | 7/11 |
| No JIT, script | 3,554.7 | Wasmi 2,981.1 | 1.211 | 6/11 |
| JIT, Wasm ES module | 22,460.1 | Node 26 21,184.9 | 1.060 | 0/11 |
| JIT vs Wasmtime | 22,460.1 | Wasmtime 19,729.1 | 1.126 | 1/11 |

Max RSS medians: quench-node 3.76 MB no JIT, 7.13 MB JIT. Lowest in both
modes; Wasm3 has 3.88 MB, Winch 11.64 MB.

Native reference on the same host (CoreMark C sources, `posix` port):
gcc -O2 about 25,000, gcc PGO about 25,200, clang -O2 about 23,700. 1.2x Node
is about 25,400, which is gcc native speed.
