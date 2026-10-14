# RSS lane (`v2-cloud-c2`), Linux x86_64, 2026-10-14

Host: Linux x86_64, 4 vCPU Intel Xeon @ 2.10GHz (KVM), 15 GiB, kernel 6.18.
Max RSS is `wait4` `ru_maxrss` from a minimal `posix_spawn` parent
(`maxrss.c`); a Python parent inflates small children by its own pages.
File-backed RSS depends on page-cache state (fault-around maps cached
neighbours of each faulting page), so comparisons interleave binaries with
the binary already cached, as repeated stock rounds have it.

## Steps

| Step | Empty `quench-node` | DeltaBlue fixed work |
| --- | ---: | ---: |
| `v2-merged` `f71a0ee24` | 44.0 MB | 43.9 MB |
| Shared atom names (`80377bf4e`) | 35.6 MB | 36.3 MB |
| + disabled-VFS deferral (`290699bce`, cherry-picked) | 32.1 MB | 32.8 MB |
| + `fs` on first use, startup intrinsics (`36c4e9cb5`) | 24.2 MB | 25.1 MB |
| + start-up-ordered layout | 17.9 MB | 21.4 MB |

QuickJS 2026-06-04 on the same host: 3.1 MB empty, 3.6 MB DeltaBlue.

## `fs` on first use: startup intrinsics

The host seals the realm's intrinsics after bootstrap, before preloads and
the entry script: own properties of the global object, of each object a
global property holds, and of its `prototype`
(`vm/startup_intrinsics.rs`). Each lazy builtin build runs with those
descriptors in place and puts the script's back afterwards, as Node's
builtins use primordials. Before this, every lazy builtin called a script's
replacement of an intrinsic while loading (`http`, `zlib`); `fs` was kept
eager only to avoid that. Oracle (`../richards-vfs-linux-2026-10-14/oracle/`
plus the same probe for `http`, `zlib`, `events`): all now match Node 26.10.0;
the other oracles are unchanged against control (three keep the gaps
recorded there).

Checks: runtime lib 584/584; Node parallel manifest, candidate and control
both pass=406 skip=10 fail=317 timeout=2 crash=0 (exit 0); all-eight
Callgrind guard (`guard-lazyfs-raw.txt`): start-up instructions -35% to -72%,
no per-run regression (Splay -17.4%, RegExp -1.7%).

| Fixture (quarter plan) | Max RSS before order | After order |
| --- | ---: | ---: |
| Crypto | 26.8 MB | 23.4 MB |
| DeltaBlue | 24.9 MB | 21.3 MB |
| EarleyBoyer | 37.5 MB | 34.0 MB |
| NavierStokes | 26.4 MB | 22.8 MB |
| RayTrace | 25.4 MB | 21.7 MB |
| RegExp | 52.3 MB | 49.4 MB |
| Richards | 24.2 MB | 20.3 MB |
| Splay | 64.9 MB | 61.1 MB |

## Start-up-ordered layout

Only 2.6-2.9 MB of code executes during an empty start, but 7-10 MB of
`.text` was resident: fault-around maps cached pages around each fault, so
executed functions scattered among cold ones keep the cold pages resident.
`crates/quench-node/link/x86_64-linux.order` lists the functions an empty
start and `quench-bench/deegen-curriculum` execute, in first-execution order
(`symbol-order` from `quench-wasm-test`, Cachegrind `--cache-sim=no
--demangle=no` on the production binary); `build.rs` links with lld and
`--symbol-ordering-file`. No scored bench is in the training set (decision 13).

Reproduce the order:

```sh
for f in empty.cjs quench-bench/deegen-curriculum/*.js; do
  valgrind --tool=cachegrind --cache-sim=no --demangle=no \
    --cachegrind-out-file=train/$(basename $f).out target/production/quench-node $f
done
target/production/symbol-order target/production/quench-node \
  crates/quench-node/link/x86_64-linux.order train/empty.cjs.out train/*.js.out
```

## Score guard for the VFS deferral, lazy `fs` and the ordered layout

`pair-fixed-work-11.json`: `quench-bench --all --fixed-work --runs 11`, base
`80377bf4e` (shared atom names), candidate with all three. Paired marginal
wall per `run()` (work minus setup-only), median and 95% bootstrap interval;
work max RSS medians:

| Fixture | Wall | Interval | RSS base | RSS candidate |
| --- | ---: | --- | ---: | ---: |
| Crypto | -4.03% | [-8.45%, +1.35%] | 38.6 MB | 21.8 MB |
| DeltaBlue | -8.69% | [-16.00%, -3.29%] | 36.6 MB | 20.2 MB |
| EarleyBoyer | -3.83% | [-12.08%, +0.82%] | 49.0 MB | 33.3 MB |
| NavierStokes | +0.92% | [-5.07%, +11.64%] | 37.6 MB | 21.4 MB |
| RayTrace | -3.72% | [-14.46%, -0.62%] | 37.5 MB | 20.5 MB |
| RegExp | -4.63% | [-15.07%, +14.54%] | 65.0 MB | 49.9 MB |
| Richards | -0.87% | [-9.33%, +3.32%] | 36.2 MB | 19.0 MB |
| Splay | -0.08% | [-6.66%, +3.55%] | 88.0 MB | 71.2 MB |

No fixture loses wall with an interval excluding zero.
