# Host operation lookup: a two-byte sorted index in place of a hash map

Host: Linux x86_64 VM, 4 vCPUs, kernel 6.18. Binaries: plain production builds of v2-gc
`78e22707f6` (A) and of the same tree plus `092cb05e2b` (B). Compile cache on, MemoryFirst.

**Finding:** `shared_vm::operation` kept an `FxHashMap<&str, u32>` from every Node binding's
name to its position, sized for every binding. massif put 50 KiB of every process's
start-up heap in it (`OnceLock::initialize` under `operation`).

**Change:** a list of the bindings' positions as `u16`, ordered by name, searched by binary
search over the bindings' own static names: 1.8 KiB.

| Measure | A | B |
| -- | --: | --: |
| `print(1)` max RSS, 4 warm runs (KiB) | 3552–3640 | 3416–3476 |
| Richards max RSS, 5 runs (KiB) | 4156–4160 | 3944–4120 |
| `console.log(1)` instructions (callgrind) | 656,241,157 | 656,830,779 (+0.09%) |
| 20-module `require` start-up, median of 15 | 87.0 ms | 89.4 ms (within run-to-run spread) |

**Behaviour:** `host-ops.js` exercises fs, crypto, zlib, URL, process members, a child process,
events, streams, an HTTP round trip and a Worker. Its output is the same from A and B and from
Node 22 (`host-ops.*.txt`). The `operation_names_are_unique` unit test passes.
