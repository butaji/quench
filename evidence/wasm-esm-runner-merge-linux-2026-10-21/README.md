# CoreMark through `quench-node` after merging v2-merged `e3875a266` (Linux x86_64, 2026-10-21)

Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits. The same host
type as `../wasm-esm-runner-rss-linux-2026-10-19`, but faster this session: that change's
bolted binary scores 3,167–3,446 (no JIT) and 22,782–24,374 (`--jit`) here, against
2,887–2,968 and 19,829–20,958 there. Scores compare only within one campaign.
Method as in `../wasm-esm-runner-rss-linux-2026-10-19` (`run-coremark-esm.sh`, 11 rounds,
every binary evicted before each run, `maxrss.c`), with the compile cache warm as a user's
second run finds it (one priming run, default cache directory).

## What the merge changed

The merge brought the host compile cache (docs/README.md, "Host compile cache"): with it warm,
programs decode instead of compiling, and decoding reaches none of a compilation's phase
boundaries, where the read-only pages are released. Unbolted static builds, max RSS KB, warm
cache:

| Build | Wasm ESM no JIT | `run-js.mjs` no JIT | `import "node:fs"` | `readFileSync` |
| --- | --: | --: | --: | --: |
| before the merge (`4eeccda5b`) | 4,312–4,324 | — | 3,564 | 3,920–4,084 |
| merged | 4,404 | 4,876–5,048 | 4,276–4,280 | 4,856 |
| + release after decoding, post-bootstrap | 3,396–3,428 | 4,060–4,076 | 3,564–3,660 | 3,820 |

With the cache off the merged build reaches 4,248–4,304 (Wasm ESM), so the cache's warm path
was the regression.

## Change

`Host::cached_program_decoded` (runtime): called after a program decodes from the host's
compile cache. `quench-node` releases there (`release_read_only_code_and_data`, the
compilation phase boundary's action) once `Host::initialize` has finished. Bootstrap's own
programs decode back to back: releasing after each raised an empty script's run from 11 to
18 ms (median of 40 warm runs), against 10.1 → 10.4 ms with the gate; `require("http")` etc.
50.4–52.4 → 52.3–53.5 ms with equal or lower max RSS. Skipping decodes inside a library load
(which releases once it ends) left the Wasm ESM runner at 3,736–3,764 KB, so the gate is
bootstrap only.

The first Wasm facade instantiation also releases once the instance exists, before the
module's own code runs (without it, 3,796–3,888 KB).

## Campaign (`coremark-esm-11.csv`, 11 rounds, bolted from this binary's own profiles)

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node, Wasm ESM** | **3,256.8** | 3,137–3,452 | **2.97 MB** |
| No JIT | quench-node, `run-js.mjs` | 3,239.9 | 2,917–3,365 | 3.43 MB |
| No JIT | Wasmi v2 | 3,096.0 | 2,942–3,322 | 5.02 MB |
| No JIT | Stitch | 2,466.3 | 2,395–2,515 | 4.32 MB |
| No JIT | Wasm3 | 2,209.3 | 2,110–2,268 | 3.82 MB |
| No JIT | WAMR | 2,080.6 | 1,939–2,180 | 4.05 MB |
| JIT | **quench-node --jit, Wasm ESM** | **24,073.2** | 22,939–26,320 | **6.49 MB** |
| JIT | quench-node --jit, `run-js.mjs` | 24,471.8 | 23,305–25,250 | 6.63 MB |
| JIT | Node v26.12.0, Wasm ESM | 23,355.4 | 22,568–26,422 | 51.91 MB |
| JIT | Node v26.12.0, `run-js.mjs` | 22,955.5 | 22,355–25,422 | 52.23 MB |
| JIT | Wasmtime | 19,062.1 | 18,311–19,614 | 13.39 MB |
| JIT | Winch | 9,527.9 | 9,192–9,896 | 11.55 MB |

Wasm ESM rows:

* **No-JIT Score: won** (1.052x Wasmi v2; ahead in 8 of 11 rounds).
* **No-JIT max RSS: won** (2.97 MB against Wasm3's 3.82 MB, 11 of 11).
* **JIT Score: won** (1.031x Node's best row; ahead of every engine in 9 of 11 rounds).
* **JIT max RSS: won** (6.49 MB against Winch's 11.55 MB, 11 of 11).

The score margins are smaller than on 2026-10-19 (1.074x, 1.065x) because the comparators
gained more from this session's host than Quench did (Wasmi +15%, Node +22%; Quench +12%,
+18%). Interleaved on this host, the merged build is no slower than the previous one (no JIT
3,124–3,456 against 3,167–3,446; `--jit` 23,483–24,622 against 22,782–24,374).

## Checks

* `child_entry` `process_warnings_match_node…` and `wasm_files_are_es_modules_as_in_node`
  pass; runtime lib 787/787; `quench-node` lib 23/23; `node_contracts`, `shared_regressions`
  pass.
* `child_entry` full run: 21 failures, all failing in the 2026-10-19 run as well except
  `cli_signal_defaults_reset_inherited_ignores_before_vm_workers_start`, which fails the same
  way on the merged tree without this change.

## After merging v2-merged `acd849678` (`coremark-esm-acd849678-11.csv`)

v2-merged moved during the campaign above (a retrained static link order, per-thread
allocator heaps, JavaScript JIT hash loops). Merged and bolted again from that binary's own
profiles; 11 rounds, same method:

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node, Wasm ESM** | **3,177.1** | 2,904–3,387 | **2.99 MB** |
| No JIT | quench-node, `run-js.mjs` | 3,150.4 | 3,036–3,295 | 3.39 MB |
| No JIT | Wasmi v2 | 3,044.6 | 2,992–3,407 | 5.10 MB |
| No JIT | Wasm3 | 2,170.0 | 2,043–2,282 | 3.84 MB |
| JIT | **quench-node --jit, Wasm ESM** | **23,616.5** | 21,753–24,341 | **6.12 MB** |
| JIT | quench-node --jit, `run-js.mjs` | 23,520.2 | 22,033–24,093 | 6.27 MB |
| JIT | Node v26.12.0, Wasm ESM | 22,419.8 | 21,296–23,381 | 51.82 MB |
| JIT | Node v26.12.0, `run-js.mjs` | 22,378.0 | 21,858–23,950 | 52.43 MB |
| JIT | Wasmtime | 18,726.6 | 17,720–19,484 | 13.28 MB |
| JIT | Winch | 9,227.4 | 9,065–9,649 | 11.43 MB |

* No-JIT Score: won (1.044x Wasmi v2; 7 of 11 rounds).
* No-JIT max RSS: won (2,852–3,060 KB against Wasm3's 3,752–4,112 KB; 11 of 11).
* JIT Score: won (1.053x Node's best row; 8 of 11 rounds, and the `run-js.mjs` row 10 of 11).
* JIT max RSS: won (6.12 MB against Winch's 11.43 MB; 11 of 11).

Interleaved `--jit` rounds before this campaign put this build at a median 23,534 against the
previous merge's 24,111 and Node's 22,381 (six rounds each); the ranges overlap.

## At `4974bb439`: startup snapshot kinds, trunk's opt-in snapshot (`coremark-esm-4974bb439-11.csv`)

v2-merged `8336df453` added a startup snapshot that preloads 14 builtins. A restore decodes
all a snapshot holds, about 5.2 MB anon with the builtins (heap 1.8, programs 1.5, shapes
0.8, atoms 0.5; `iteration` build, empty entry), so CoreMark as a Wasm ES module rose from
3.3 to 7.7 MB unbolted. This lane added a `bootstrap` kind restored when the entry's static
graph cannot reach a builtin (docs/README.md, "Startup snapshot"); trunk meanwhile made the
snapshot opt-in (`QUENCH_STARTUP_SNAPSHOT=on|trace`). The campaign runs the default, without
a snapshot.

The host was slower this run: interleaved `--jit` rounds put the previous campaign's binary at
19,338 and this one at 19,371 (Node 18,698); no JIT 2,865 and 2,749, ranges overlapping.

| Mode | Runtime | Median Score | Range | Median max RSS |
| -- | -- | --: | -- | --: |
| No JIT | **quench-node, Wasm ESM** | **2,841.9** | 2,613–2,939 | **2.93 MB** |
| No JIT | quench-node, `run-js.mjs` | 2,894.8 | 2,764–2,923 | 2.91 MB |
| No JIT | Wasmi v2 | 2,639.7 | 2,611–2,728 | 5.02 MB |
| No JIT | Wasm3 | 1,821.5 | 1,706–1,859 | 3.82 MB |
| JIT | **quench-node --jit, Wasm ESM** | **20,242.9** | 18,647–21,030 | **6.27 MB** |
| JIT | quench-node --jit, `run-js.mjs` | 19,977.4 | 18,972–20,594 | 6.31 MB |
| JIT | Node v26.12.0, Wasm ESM | 19,291.4 | 18,163–20,076 | 51.82 MB |
| JIT | Node v26.12.0, `run-js.mjs` | 19,191.4 | 18,409–20,233 | 52.31 MB |
| JIT | Wasmtime | 18,174.1 | 17,153–18,447 | 13.34 MB |
| JIT | Winch | 9,232.1 | 8,538–9,592 | 11.56 MB |

* No-JIT Score: won (1.077x Wasmi v2; 10 of 11 rounds).
* No-JIT max RSS: won (2,960–3,020 KB against Wasm3's 3,708–4,068 KB; 11 of 11).
* JIT Score: won (1.049x Node's best row; 9 of 11 rounds).
* JIT max RSS: won (6.27 MB against Winch's 11.56 MB; 11 of 11).

Max RSS of this bolted binary with and without the snapshot (KB, 3 runs, evicted):

| Entry | Snapshot kind | Off | On |
| -- | -- | --: | --: |
| empty `.mjs` | bootstrap | 2,024 | 2,232 |
| CoreMark Wasm ESM | bootstrap | 3,000–3,052 | 3,072 |
| CoreMark Wasm ESM `--jit` | bootstrap | 6,300–6,528 | 6,208–6,324 |
| `require("http", "util", "stream")` | preloaded | 9,844–9,980 | 7,592–7,720 |

Restoring the bootstrap kind costs about 200 KB over booting for an empty script, so the
opt-in default keeps CoreMark's lowest figure; an entry that loads the preloaded builtins
saves about 2.3 MB with the snapshot on.
