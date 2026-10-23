# TOD-167: the global allocator follows the memory policy — kept

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. Both sides are static `quench-node` builds,
each bolted with a BOLT profile retrained on its own code (`train2.sh` on that binary, then
`bolt-profile` against the repository link order):

| Side | Source |
| -- | -- |
| Base (`qb7`) | v2-gc `3d7dc0656a` |
| Candidate (`qpa9`) | `cand-gc-policy-allocator` |

Binaries are evicted before each run, the compile cache is warm, and pairs run in alternating
order.

## Change

- quench-alloc carves every chunk from one reservation made once per process (64 GiB,
  `PROT_NONE`, made writable one chunk at a time). `owns(ptr)` is then one address compare.
- Frees and reallocations route by ownership instead of by layout, so a block always returns
  to the allocator that made it.
- Small requests go to quench-alloc only after `QuenchAlloc::enable()`. Until then, and
  whenever no page can be had, they go to glibc.
- `shared_run::apply_allocator_policy`, the process's single policy selection point, enables
  quench-alloc for Throughput. MemoryFirst never enables it.
- quench-node installs the allocator on every Unix build. The `quench-alloc` Cargo feature
  and its `default` toggle are removed.
- `alloc_zeroed` keeps glibc's `calloc`, so large zeroed requests stay fresh zero pages.
- The disabled path costs one load and a jump: the enabled path is a separate, non-inlined
  function, so the disabled path builds no stack frame.

## Gate

### MemoryFirst: the allocator stays glibc's

V8-v7 (`v8v7-bolted-both-policies-7-runs.txt`):

- Max RSS changed by −116 to +104 KiB per row; the A/A floor is about ±130 KiB on these
  binaries.
- Scores are within noise.

Callgrind on Ajv (an npm scenario), warm cache:

| Build | Instructions |
| -- | -- |
| Base | 265.9M |
| First candidate | 269.3M (the size class was computed before the switch was checked, and the fast path's stack frame was paid even when disabled) |
| Final candidate | 267.1M (+0.4%) |

JetStream (`jetstream-subset-5-runs.txt`): scores ×0.97–1.01, RSS equal.

npm (`npm-9-runs.txt`): RSS equal. Times are within noise, except Ajv, which reads +10% in
wall time against +0.4% in instructions.

### Throughput: the allocator becomes quench-alloc

V8-v7:

- Scores: RegExp ×1.16 without JIT and ×1.09 with it; EarleyBoyer ×1.08 and ×1.09. The rest
  are within noise.
- Max RSS: RegExp −1076 KiB without JIT. Otherwise −204 to +472 KiB, with the largest rises
  on Crypto (+268 / +344) and NavierStokes `--jit` (+472).

JetStream subset:

| Test | Score | RSS |
| -- | -- | -- |
| bigint-noble-ed25519 | ×2.00 | 35.5 → 32.7 MB |
| bigint-paillier | ×1.32 | 33.9 → 35.3 MB |
| json-parse-inspector | ×1.15 | equal |
| stanford-crypto-aes | ×1.035 | 35.7 → 39.3 MB |

The aes RSS rise matches jet's board (31.9 → 37.3).

npm: TypeScript −9% wall time; ESLint, Acorn and Ajv +1 to +3%. RSS is up to 2 MB higher.

Tests:
- `cargo test -p quench-alloc`: 7 passed, including new ownership and zeroed tests. Base and
  candidate each passed 25 of 25 repeated runs; one failure in an earlier run did not
  reproduce.
- `cargo test -p quench-node --lib`: 27 passed.

Bolt profiles must be trained on the binary they lay out. The final candidate bolted with
the first candidate's profile read +228 to +376 KiB on every fixture under MemoryFirst. That
is layout, not memory.
