# Sparse per-iteration environment copies (2026-10-18)

Host: `host.txt` (x86_64 Linux 6.18.44 VM, 4 CPUs, 16 GiB RAM). Comparators: Node v26.12.0 and
Bun 1.4.2 (`BUN_OPTIONS=--smol`). Base: v2-merged 7fb9e1853.

## Finding

A loop with `let` bindings in a function whose environment is captured copies that
environment each iteration (`CloneEnv`). Each copy was dense, one word per slot of the whole
function, with every non-fresh slot pointing at its owner.

Bundles keep thousands of module-level bindings in one environment, and top-level loops make
closures that keep their iteration's copy alive:
- **playwright:** 803 copies of at least 1,000 slots, holding 2.88M slots, of which 2.56M were
  shared pointers (about 22 MiB).
- **typescript-eslint:** 87 such copies, holding 532k shared pointers.

## Fix

A copy of an environment of `SPARSE_CLONE_MIN_SLOTS` (64) or more slots stores only the slots
that differ from sharing one default owner: `[len, shared(owner), (index, slot)...]`.
- The thin slice header's mark tells the two layouts apart.
- The index words are `TAG_DELETED` payloads (`Value::environment_index`), which no guest
  operation produces.
- Every access goes through `EnvironmentSlots::state`, `owned`, `owned_mut` and `roots`.
- The lane's cached capture pointer holds dense storage only (`EnvironmentSlots::dense`), so
  its hot read tests nothing more than before.

Semantics: `loopclone.js` (300 function-level bindings, closures over per-iteration `let`
bindings, nested loops, writes through shared slots) prints the same as Node.
`wide_environment_clones_store_only_the_slots_they_do_not_share` covers values, writes, copies
of copies, collection and narrow copies.

## Effect (release builds; all 300 pass)

| package | base | candidate | Node | Bun |
| --- | --- | --- | --- | --- |
| playwright | 135.6 | 118.4 | 136.7 | 99.0 |
| typescript-eslint | 188.4 | 184.6 | 230.9 | 147.4 |
| @typescript-eslint/type-utils | 224.2 | 220.4 | 203.0 | 123.2 |

Lowest of 300: 283 (281 at base); median Quench 19.5 MiB. `rss-report.txt` and
`npm-scenarios.json.gz` are this run.

## Instruction guard (`callgrind-guard.txt`)

Per-run instructions change by 0.00% to +0.31% (earley-boyer; the sparse-aware write path
through `environment_slot_mut`).

Two intermediate versions measured higher and were reworked:
- **+4.8%:** a layout test on slot 0 for every read, and filtering every slot during marking.
- **+1.0%:** a mark test inlined into the lane's capture read.
