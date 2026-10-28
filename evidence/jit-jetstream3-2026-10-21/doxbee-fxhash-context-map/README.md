# Async context map hasher screen

Promise async IDs and private job tokens are monotonic, runtime-generated
integers. A candidate replaced the standard randomized hasher with
`rustc_hash::FxHashMap` / `FxHashSet` for the internal async ID, job-token, and
weak-resource maps. Guest-keyed AsyncLocalStorage maps were left unchanged.

Sequential three-round screens produced mixed scores:

| Fixture | Standard hasher score / RSS | Fx hasher score / RSS |
| --- | ---: | ---: |
| `doxbee-promise` | 3.07 / 170,000,384 B | 3.60 / 170,192,896 B |
| `doxbee-async` | 5.19 / 257,392,640 B | 3.22 / 251,445,248 B |

Promise score moved up, Async score moved down, and peer score medians changed
between runs. Async RSS fell 2.3%; Promise RSS was flat. These sequential
results do not establish a score win, and RSS did not improve consistently, so
the hasher change was reverted. The reports preserve all per-round results and
binary hashes; a paired, randomized screen would be needed before revisiting.

- `std-hash-3round.json`
- `fxhash-3round.json`
