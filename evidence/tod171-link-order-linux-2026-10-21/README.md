# TOD-171: retrained quench-node link order

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Production profile `quench-node`, both
binaries run with `QUENCH_COMPILE_CACHE=`.

- `p0`: v2-gc at d2063ae67, linked with the existing order.
- `p1`: the same code linked with the retrained order. Only the layout differs.

## Finding

`crates/quench-node/link/x86_64-linux.order` was last retrained in a9538cfb0
(2026-10-18). Only 206 of its 3,469 symbols still exist in today's production binary. The
crate hashes in the mangled names changed since, so production builds linked effectively
unordered.

## Change

Retrained with the TOD-123 recipe (`train.sh`, `sample262.py`):
- Cachegrind (`--cache-sim=no --demangle=no`) on the production binary.
- Training set: an empty start, `quench-bench/deegen-curriculum`, `quench-bench/micros`, and
  a 300-file Test262 sample (`random.seed(13)`, the recipe's filters).
- `symbol-order` from `quench-wasm-test`.
- No scored bench is in the training set.

Result: 402 profiles and 2,913 ordered symbols, all present in the binary.

## Result: kept

Rule (TOD-171):
- npm ≤ ×0.995 or a lower empty-start RSS.
- No suite's RSS above ×1.005 and no suite's speed below ×0.99.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| npm top 300 | 1 | — | **×0.9714**, none worse by 5%, statuses unchanged |
| V8-v7 | 10 | ×1.014 | **×0.936** |
| Frameworks | 5 | ×1.095 | ×0.980 |
| JetStream (7 tests) | 5 | ×1.019 | ×0.983 |

Empty start: about 4.04 → 3.67 MB. Richards: 4.32 → 3.90 MB.

## Keeping it fresh

The order names symbols with the profile's crate hashes. A change to the crate's metadata
(dependencies, manifest) silently invalidates it, so check the match rate after such
changes:

```sh
nm target/production/quench-node | awk '{print $NF}' | sort -u > syms
sort -u crates/quench-node/link/x86_64-linux.order | comm -12 - syms | wc -l
```

The static qualification order (`x86_64-linux-static.order`) needs the same retrain on its
own build.
