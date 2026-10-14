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
start, `quench-bench/deegen-curriculum` and `quench-bench/micros` execute, in first-execution order
(`symbol-order` from `quench-wasm-test`, Cachegrind `--cache-sim=no
--demangle=no` on the production binary); `build.rs` links with lld and
`--symbol-ordering-file`. No scored bench is in the training set (decision 13).

Reproduce the order:

The Test262 sample (`train-test262/`) is up to 30 files from each of `test/language/{expressions,statements,function-code}` and `test/built-ins/{Array,String,Object,Function,Math,JSON,Number}`: files without `includes:`, `negative:` or the `module`, `async`, `raw`, `onlyStrict` and `CanBlock` flags, in Python `random.seed(13)` shuffle order, each prefixed with `harness/assert.js` and `harness/sta.js`.

```sh
for f in empty.cjs quench-bench/deegen-curriculum/*.js quench-bench/micros/*.js train-test262/*.js; do
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

## Lazy web globals and `Temporal`

The web-stream family, the web-API group, `URL`/`URLSearchParams`,
`TextDecoder`/`TextEncoder` and the runtime's `Temporal` became lazy data
properties (`docs/node-host-contracts.md`, "Lazy web globals"). Stock V8-v7
max RSS, two interleaved rounds each, both binaries with a freshly trained
order (base `ea5411412`):

| Fixture | Base | Candidate | QuickJS |
| --- | ---: | ---: | ---: |
| Crypto | 12.0 MB | 10.6 MB | 3.5 MB |
| DeltaBlue | 11.5 MB | 10.3 MB | 3.8 MB |
| EarleyBoyer | 17.9 MB | 16.3-16.7 MB | 16.8 MB |
| NavierStokes | 12.5 MB | 10.7 MB | 5.0 MB |
| RayTrace | 11.3 MB | 10.0 MB | 3.3 MB |
| RegExp | 23.0 MB | 21.9 MB | 6.8 MB |
| Richards | 11.1 MB | 9.7 MB | 3.3 MB |
| Splay | 55.4 MB | 54.7 MB | 147 MB |

Empty start: 9.4 MB to 7.9 MB. All-eight Callgrind guard
(`guard-lazy-globals-raw.txt`): start-up instructions -8% (RegExp) to -57%
(Richards); per-run within ±2.6% except Splay +21.8% (15.1M to 18.3M
instructions per run). Splay runs the same 13 collections, but at points with
more live tree nodes (object visits 1.48M to 1.94M over 75 runs): GC phase,
not mutator work; whole-program instructions fall 1.0%, and Score stays within
run noise. The upstream `test/parallel` manifest gives the same outcome for
every case (405 pass), and the Temporal and `global` Test262 directories pass
6669 of 6669 on both binaries.

At peak a small fixture holds about 6 MB of file pages (our `.text` 3.5 MB,
read-only data 1.8 MB, libc 0.6 MB per bootstrap phase). The kernel maps a
cached file folio around each fault: a stride of one touch per 256 KB over the
binary maps 141 KB per touch when the page cache came from readahead, 4.6 KB
when it holds single pages. Releasing clean `.rodata` and RELRO pages at the
phase boundaries as well re-faulted them at that granularity and raised the
empty start by 0.5 MB, so only code mappings are released.

## Static C runtime qualification build

The documented harness recipe (docs/README.md, "Wasm execution modes and link
order") applied to `quench-node`: `build.rs` resolves the frame-registration
hooks to null and links `link/x86_64-linux-static.order`, trained on the static
binary with the corpus above. Production builds stay dynamic.

```sh
RUSTFLAGS="-D warnings -C llvm-args=-align-all-functions=6 \
  -C target-feature=+crt-static -C relocation-model=static" \
  CARGO_TARGET_DIR=target/static cargo build --profile production \
  --target x86_64-unknown-linux-gnu -p quench-node --bin quench-node
```

Stock V8-v7 max RSS, two interleaved rounds against the dynamic build of the
same source (MB): Crypto 10.4 -> 9.4, DeltaBlue 10.2 -> 9.2, EarleyBoyer 16.3
-> 15.4, NavierStokes 10.4 -> 9.4, RayTrace 9.8 -> 8.9, RegExp 21.6 -> 20.3,
Richards 9.4 -> 8.4, Splay 54.5 -> 53.9; empty start 7.7 -> 7.1. Only the libc
code a run executes is resident, laid out with the rest. Callgrind
(`guard-static-runtime-raw.txt`): start-up -3% to -5%, per-run -2.5% (RegExp)
to -5.7% (Crypto). DNS (local and remote `dns.lookup`), credential lookups,
`os.homedir`, crypto, zlib, child processes and a local HTTP round trip behave
as in the dynamic build.
