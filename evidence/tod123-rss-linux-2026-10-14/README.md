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

## Run-phase layout

A small fixture's peak comes after start-up: the run and its output touch a
few hundred functions spread through the start-up-ordered layout, and each
touch maps cached neighbours (64 KB fault-around, whole folios when cached
large), so the run kept 4-6 MB of file pages resident. `quench_entry_compiled`
(a no-op called once the entry has compiled) splits a callgrind profile there
(`--dump-before`); `symbol-order --run` places the functions executed after it
first: the empty start's in order, then the corpus's by how many programs
executed them; then every per-instruction lane handler (`--dense
13dispatch_fast`, 126 KB of small functions a hot loop reaches by its code,
which short training programs rarely do); then the Test262 sample's run
functions, ranked as a second tier so its builtin coverage does not push the
interpreter's paths apart. The start-up order follows. Retrain both orders (static and
dynamic builds) on their own binaries:

```sh
for f in empty.cjs quench-bench/deegen-curriculum/*.js quench-bench/micros/*.js train-test262/*.js; do
  valgrind --tool=cachegrind --cache-sim=no --demangle=no \
    --cachegrind-out-file=train/$(basename $f).out $BINARY $f
  valgrind --tool=callgrind --dump-instr=yes --dump-line=no --demangle=no \
    --dump-before=quench_entry_compiled \
    --callgrind-out-file=run/$(basename $f).out $BINARY $f
done
# Wasm: the interpreter's handlers, then the compiler's pipeline (never a scored module).
for mode in no-jit jit; do
  valgrind --tool=cachegrind --cache-sim=no --demangle=no \
    --cachegrind-out-file=train/wasm-kernels-$mode.out \
    $BINARY $([ $mode = jit ] && echo --jit) quench-bench/wasm-micros/kernels.cjs 3
done
# The static order adds --foreign-first (block layout below).
target/production/symbol-order $BINARY $ORDER --dense 13dispatch_fast \
  train/empty.cjs.out train/*.js.out \
  train/wasm-kernels-no-jit.out train/wasm-kernels-jit.out \
  --run run/empty.cjs.out run/{curriculum,micros}*.out --run run/test262-*.out
```

Stock V8-v7 max RSS against the previous orders (MB), static / dynamic:
Crypto 10.0 -> 9.0 / 11.1 -> 10.4, DeltaBlue 9.8 -> 9.1 / 10.9 -> 10.2,
EarleyBoyer 15.6 -> 15.0 / 17.0 -> 16.1, NavierStokes 9.9 -> 9.1 / 11.1 ->
10.5, RayTrace 9.2 -> 8.4 / 10.1 -> 9.8, RegExp 21.3 -> 21.2 / 22.0 -> 22.0,
Richards 8.9 -> 8.1 / 9.9 -> 9.4, Splay 54.2 -> 53.8 / 55.1 -> 55.0. The
empty start rises 0.6 MB (start-up code now follows the run region); no
fixture peaks during start-up.

## Block layout (BOLT)

Function order cannot pack a run's code below the size of the functions it
enters: Richards' run phase executes 59,000 distinct instructions (about 235 KB)
spread over 346 pages, 45 fault-around windows of the run region. The static
qualification binary keeps its link relocations (`--emit-relocs`) and is
rewritten by `llvm-bolt` 20 from a basic-sample profile of the same training
corpus (`bolt-profile`, from the order's callgrind profiles taken with
`--dump-instr=yes --dump-line=no`): each function's executed blocks move into
hot text in the trained function order, the never-executed ones into cold text.
Commands: `docs/README.md`, "Wasm execution modes and link order".

Three choices decide the result; each was measured against the alternative:

- The interpreter dispatch (`run_frame_general_until`, the lane handlers) stays
  whole (`--dense`). Split by the corpus, a scored run reached 16 pages of its
  cold blocks.
- BOLT leaves code not generated from Rust in place: glibc's single-thread
  atomics jump over a `lock` prefix, which BOLT splits off its instruction
  (`lock jmp`, `SIGILL` in `_int_free`). `symbol-order --foreign-first` packs
  that code at the start of the original text; left among the moved Rust
  functions it touched 21 windows for 44 pages.
- Splitting stays on and the profile covers start-up as well as the run: no
  split, a run-phase-only profile, `-jump-tables=move` and `cdsort` function
  order each measured 0.1-0.5 MB worse.

Stock V8-v7 max RSS, static build of the same source without and with BOLT,
binaries evicted from the page cache first (MB): Crypto 8.0 -> 7.5, DeltaBlue
7.5 -> 7.4, EarleyBoyer 14.2 -> 14.3, NavierStokes 8.1 -> 7.7, RayTrace 7.2 ->
6.7, RegExp 19.0 -> 18.8, Richards 6.8 -> 6.4, Splay 52.2 -> 51.9. Richards'
run touches 238 text pages in 38 windows instead of 346 in 45.

A just-written binary is cached in large folios, whose pages fault-around maps
together: the first BOLT output measured 15.4 MB on Richards until evicted
(`posix_fadvise(POSIX_FADV_DONTNEED)`), then 7.3 MB. Comparisons evict every
binary first.

Correctness: 529 Node parallel fixtures (every eighth) run directly on both
binaries exit identically, except `test-webcrypto-digest-turboshake-rfc.js`,
which times out at 20 s without BOLT and fails with it (it fails either way).

Callgrind guard (`guard-bolt-raw.txt`, without against with BOLT): start-up
-0.4% to -1.0%, per-run -1.15% (RayTrace) to +0.80% (NavierStokes).

## Packed shapes, packed seal, lazy process members

A shape kept its descriptor's getter and setter inline (80 bytes); accessor
functions now sit in a side table that shape compaction rebuilds, and a shape
is 48 bytes. The startup intrinsic records reuse that packing (72 -> 40
bytes). The process surface script's members no start needs (getBuiltinModule,
cpuUsage, memoryUsage, release, setSourceMapsEnabled, ...) are a lazy data
property group on `process`. Node parallel manifest unchanged (pass=442).

Stock V8-v7 max RSS, BOLT static builds before and after (MB, two interleaved
rounds, evicted): Crypto 7.6 -> 7.3, DeltaBlue 7.4 -> 7.2, EarleyBoyer 14.0 ->
13.7, NavierStokes 8.0 -> 7.6, RayTrace 6.9 -> 6.6, RegExp 19.2 -> 18.8,
Richards 6.7 -> 6.4, Splay 52.1 -> 51.7. Callgrind
(`guard-shapes-process-raw.txt`): start-up -0.8% (RegExp) to -12.5%
(Richards), per-run -0.43% to +0.01%.

## Touched pages only (userfaultfd write protection)

Fault-around maps a faulting page's cached 64 KB neighbourhood (or a whole large
folio) into the process, so code and constants a run never touches stayed
resident beside what it touches: a small fixture kept 4-6 MB of file pages for
about 1.5 MB it uses. The kernel skips fault-around in a mapping registered
with userfaultfd for write protection; quench-node registers its read-only
file mappings at start with asynchronous write protection
(`UFFD_FEATURE_WP_ASYNC`, Linux 6.7), which needs no handler and which these
never-written mappings never trigger (`resident_pages::map_touched_pages_only`).
Without the feature the mappings keep fault-around.

Same static binary without and with the registration (MB): Richards 7.5 -> 4.2,
DeltaBlue 8.0 -> 4.7, RayTrace 7.9 -> 4.3, Crypto 8.6 -> 5.1. Start-up minor
faults 1215 -> 3929, wall unchanged within noise (median 23-31 ms either way),
Scores unchanged within noise. 529 Node parallel fixtures run directly on the
binary exit identically (207 pass both ways). A freshly written binary now
measures 0.3 MB above the same file evicted, against 8 MB with fault-around.

Stock V8-v7 max RSS after retraining both orders and the BOLT profile (MB;
static build, then BOLT; QuickJS 2026-06-04, Bun and Node without JIT, same
host):

| Fixture | Quench | Quench BOLT | QuickJS | Bun | Node | Bun JIT | Node JIT |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Crypto | 5.0 | 4.5 | 3.5 | 22.0 | 44.2 | 45.9 | 60.0 |
| DeltaBlue | 4.7 | 4.2-4.4 | 3.5 | 22.5 | 47.0 | 43.2 | 84.8 |
| EarleyBoyer | 11.2 | 10.8-10.9 | 17.0 | 26.9 | 110.3 | 72.4 | 129.4 |
| NavierStokes | 5.3 | 5.0-5.2 | 5.0 | 22.0 | 49.3 | 36.3 | 59.5 |
| RayTrace | 4.4 | 4.0 | 3.3 | 23.2 | 51.3 | 54.5 | 67.5 |
| RegExp | 15.4 | 14.9 | 6.6 | 30.5 | 55.1 | 52.6 | 70.7 |
| Richards | 4.0 | 3.5 | 3.4 | 21.9 | 44.8 | 39.2 | 56.8 |
| Splay | 49.3 | 49.0 | 147.0 | 62.5 | 395.6 | 188.4 | 460.8 |

Callgrind (`guard-touched-pages-raw.txt`, previous publication against this
one, across a v2-merged sync): start-up -2.0% to -12.6% except NavierStokes
+1.3% and RegExp +1.7%; per-run -25% (Richards) to +2.6% (RegExp), most of the
per-run change from the merged upstream work.

## One-word string aside, retrained orders

A string's rope halves (present only until it flattens) and a wide string's host text (made
only after) share one `Aside` slot, shrinking every string allocation. Unbolted static A/B
(MiB, 3 evicted runs, medians): RegExp 15.18 → 14.64, Splay 49.26 → 48.77, DeltaBlue
4.45 → 4.26, Richards 3.94 → 3.90; Crypto, NavierStokes, RayTrace within +0.1 (layout).
Both symbol orders were retrained on the packed-seal/touched-pages code.

Guard (`guard-strings-orders-raw.txt`, base `origin/v2-merged` d1c09c900, candidate its merge
with this batch): per-run deltas Crypto −0.00%, DeltaBlue +0.23%, EarleyBoyer +0.34%,
NavierStokes −0.00%, RayTrace +0.95%, RegExp +0.79%, Richards −0.00%, Splay −0.04%.
Matched unbolted maxrss (MiB, median of 3): Crypto 7.43 → 7.06, DeltaBlue 7.06 → 6.53,
EarleyBoyer 13.48 → 13.21, NavierStokes 8.14 → 7.87, RayTrace 6.72 → 6.56, RegExp 17.09 →
16.85, Richards 6.43 → 6.07, Splay 50.78 → 50.69.

Both sides start about 2 MiB higher than this batch before the merge: the trunk's start-up
rose from about 67M to 712M instructions (Richards N=0), now under investigation.

## Node primordials on first use

The trunk embeds the pinned Node library, so `primordials.js` became available, and ran, at
every start (Richards N=0 instructions 67M → 712M, every fixture about 2 MiB more resident).
Only Node's internal modules and the emitter async resource read the snapshot; it now builds
on first use inside the sealed startup intrinsics. The async-hook registry keeps the plain
`Set` it always received (it initializes before the snapshot existed).

Guard (`guard-lazy-primordials-raw.txt`, base merge 710f759bc, candidate e3ea30380):
start-up −640M instructions on all eight; per run Crypto +0.22%, DeltaBlue +0.67%,
EarleyBoyer +1.32%, NavierStokes 0.00%, RayTrace +0.98%, RegExp −1.61%, Richards −0.01%,
Splay −20.62% (a smaller live heap moves the trunk-owned collection points).
Matched unbolted maxrss (`board-lazy-primordials.txt`, MiB, medians): Crypto 6.94 → 5.08,
DeltaBlue 6.54 → 4.65, EarleyBoyer 13.08 → 11.19, NavierStokes 7.83 → 5.45, RayTrace
6.43 → 4.30, RegExp 16.89 → 14.90, Richards 6.08 → 4.25, Splay 50.76 → 49.00.
Node `framework-core` gate: 43/43.

## Folded data attributes, inline short strings, shared regexp matcher

- A data property's creation attributes ride on its `Add` transition: a builtin realm needs
  615 fewer shapes and transitions (2486 → 1871), and the shape table starts at a capacity
  the allocator maps on its own, so growth remaps in place.
- ASCII text of at most 22 bytes sits inline in `FlatUnits` (still three words): most names,
  tokens and substrings cost one allocation. The atom sequence holds atom strings, not
  `Rc<str>` copies; short RegExp subjects skip the identity cache.
- A regexp's range-only queries use its compiled matcher (`RangeBackend::Compiled`) instead
  of a clone with its own cache pool.

Test262 stage 10 (11087 passed, 15 waived) and stages 29–92 (23633 passed, 35 waived), no
failures; Node `framework-core` 43/43.

Guard (`guard-shapes-strings-raw.txt`, base `origin/v2-merged` bc9e0080c, candidate its merge
with this batch): per run Crypto +0.09%, DeltaBlue −0.61%, EarleyBoyer −0.22%, NavierStokes
0.00%, RayTrace +0.35%, RegExp −1.72%, Richards 0.00%, Splay −0.02%. Matched unbolted
maxrss in `board-shapes-strings.txt`.

Bolted static against QuickJS 2026-06-04 (`board-bolt-vs-qjs-b17.txt`, evicted, 7 runs
interleaved, medians KiB): Richards 3328 vs 3320, DeltaBlue 3692 vs 3680, NavierStokes 4628
vs 4948, RayTrace 3456 vs 3348, Crypto 4116 vs 3484; EarleyBoyer 10268 vs 16716, Splay 48152
vs 147000, RegExp 13176 vs 6708 (5 runs).

## Scoped bootstrap roots, transition index, thread cache, lazy Node library modules

- Bootstrap steps run as `NativeContext::scoped` steps: at most about 300 handles are live at
  once instead of about 1100 (root table and context list 73 → 24 KiB).
- A module's file text is released before its wrapper compiles.
- Cached shape transitions are a `hashbrown::HashTable` of child ids, confirmed against the
  shape table: 5 bytes an entry instead of 25–32.
- The static build limits glibc's per-thread cache to two chunks per size class (one-time
  re-execution with `GLIBC_TUNABLES`, removed from the guest environment); profiling and
  guard runs set the tunable themselves (valgrind does not follow the re-execution).
- Pinned Node library modules load on first use inside the startup intrinsics. The trunk had
  started loading `internal/util/inspect.js` at every start: about 1G instructions and
  9 MiB on every fixture (`board-trunk-eager-inspect.txt`: Richards 13.1 MiB, both sides
  unbolted).

Bolted static against QuickJS before the trunk merge (`board-bolt-vs-qjs-b20.txt`, 5 runs,
medians KiB): Richards 3084 vs 3360, DeltaBlue 3496 vs 3632, RayTrace 3328 vs 3388,
NavierStokes 4516 vs 4924, EarleyBoyer 9876 vs 16632, Splay 47544 vs 147012; Crypto 3792
vs 3504 and RegExp 12832 vs 6796 remain.

Guard (`guard-lazy-library-raw.txt`, base `origin/v2-merged` 5b9f578cb with the eager
inspector, candidate this batch): start-up −1.0G to −1.1G instructions on every fixture; per
run Crypto +0.03%, DeltaBlue +0.08%, EarleyBoyer +3.54%, NavierStokes +0.02%, RayTrace
+1.84%, RegExp +0.31%, Richards −0.07%, Splay −0.58%. Per function, EarleyBoyer's run
differs by +11M in transition lookups (the trunk's constructed-shape transitions consult
the index on every construction) and by collector work whose timing moves with the
lighter start; Test262 stage 10 and stages 29–92 and Node `framework-core` 43/43 pass.

## Node's stack policy on first use

- The trunk had started loading `internal/errors.js` and its requirements at every start for
  Node's stack policy: about 0.8G instructions and 5.5 MiB on an empty script (9120 KiB
  against 3484 KiB, static unbolted). The default stack formatter is now a native
  trampoline that loads Node's `prepareStackTraceCallback` on its first call;
  `Error.prepareStackTrace` is a lazy data property whose initializer loads the module
  inside the startup intrinsics and keeps its private brand as loaded.
- A library module builds the primordials before entering the module cache, which
  materializes the property first, so a script that requires `internal/errors` itself gets
  the module the initializer loaded.

Bolted static against QuickJS after the trunk merge (`board-bolt-vs-qjs-b23.txt`, 3 runs,
KiB): Richards 3240 vs 3328–3428, DeltaBlue 3624 vs 3736–3896, RayTrace 3464 vs 3444–3520,
NavierStokes 4548 vs 5028–5052, EarleyBoyer 9912–10040 vs 16048–16708, Splay 47844–48008
vs 146948–147052; Crypto 4052–4180 vs 3424–3672 and RegExp 13212–13228 vs 6644–6816 remain.

Guard (`guard-lazy-errors-raw.txt`, base `origin/v2-merged` f79a94a74, candidate this
batch, both static unbolted): start-up −0.79G to −0.89G instructions on every fixture; per
run Crypto +0.27%, DeltaBlue −0.85%, EarleyBoyer −0.81%, NavierStokes 0.00%, RayTrace
+1.79%, RegExp −3.69%, Richards −0.09%, Splay +0.05%. RayTrace's +8M per run is collector
work (`active_shape_attributes` +9M) whose timing moves with the lighter start; its whole
run is 2.30G → 1.50G. Node `framework-core` 43/43; `child_entry` error tests match the
trunk's failures (Node 22 oracle) except the trunk's new call-site test, whose remaining
differences are Node 27 validator messages.

## Internal policies on first use, single-copy shape lookup index

- The trunk's buffer and call-site policies loaded `util.js`, `internal/errors.js` and
  `internal/validators.js` at every start (13776 against 3536 KiB on an empty script, static
  unbolted, base `origin/v2-merged` b7723be8e). Only a script given `--expose-internals` can
  replace those exports before a policy's first use, so only then are the policies captured
  before it runs; otherwise each loads on first use.
- A shape's lookup index kept every (key, slot) pair twice; its key map is now a table of
  positions into the ordered entries (421 → 219 KiB on an empty script).

Bolted static against QuickJS (`board-bolt-vs-qjs-b24.txt`, 3 runs, KiB): Richards 3260 vs
3304–3484, DeltaBlue 3640–3644 vs 3596–3856, RayTrace 3484–3488 vs 3488–3524, NavierStokes
4680–4684 vs 4996–5096, EarleyBoyer 9912–10084 vs 16264–16828, Splay 47908–48024 vs
146944–147036; Crypto 4048–4132 vs 3460–3612 and RegExp 12964–13236 vs 6656–6844 remain.

Guard (`guard-lazy-policies-raw.txt`, base b7723be8e): start-up −1.28G to −1.36G
instructions on every fixture; per run Crypto −0.01%, DeltaBlue −0.23%, EarleyBoyer −0.06%,
NavierStokes 0.00%, RayTrace +2.21%, RegExp −3.44%, Richards −0.09%, Splay −0.72%.
RayTrace's +14M per run is collector work only (219M → 259M per three runs; the rest
1707M → 1703M): the base collects 89M during its heavier start-up. Its whole run is 3.30G →
2.05G. Runtime lib 660/660, Test262 stages 29–92 pass, `framework-core` 43/43, `child_entry`
error/buffer/shadow/call-site failures equal the trunk's. Stage 10's
`dynamic-import/usage-from-eval.js` fails on the trunk's runtime as well (missing module
fixture under eval).

## Host regexes no longer widen the JS regex engine

- `quench-node`'s host regexes (ANSI stripping, `path.matchesGlob`) took the `regex` crate
  with default features; feature unification gave every crate-backed JavaScript RegExp a
  lazy DFA and its caches, and the binary the full Unicode tables. Neither host pattern
  needs them; `quench-node` now asks for `quench-regexp`'s feature set.

Bolted static, retrained orders (`board-bolt-b25-trimmed-regex.txt`, 3 runs, KiB): RegExp
11664–11676 (was 12964–13236), Crypto 4008–4032, DeltaBlue 3632–3760, EarleyBoyer
9876–9912, NavierStokes 4560–4688, RayTrace 3468–3472, Richards 3248, Splay 48008–48020.

Guard (`guard-trimmed-regex-raw.txt`, base the previous candidate 89d86f247, candidate
this commit after the trunk merge): per run RegExp +7.78% — its searches move from the
lazy DFA (−170M) to the bounded backtracker (+381M) and its visited-set clearing (+56M);
the wall-clock score is unchanged (538 against 528). DeltaBlue +0.47%, others within
±0.13%. NavierStokes' start-up +32M is the trunk's indexed-store exotic checks
(`prototype_chain_has_indexed_set_exotic`, c21d556db) merged in between. The trade buys
1.3 MiB of RegExp's resident set.

## Array element payload charged toward the next collection

- A collection is due after a weighted count of allocations; a string's flat units already
  weighed one allocation per 64 bytes, but an array's elements weighed nothing however far
  they grew. Array elements now weigh by the same rule: their capacity at allocation and
  each growth of the backing vector (indexed stores, `push`), remainders carried forward.
  The headroom factors (decision 21) are unchanged.

Paired unbolted runs against the previous build (decision 5): Splay 48330 → 42430 KiB,
DeltaBlue 4052 → 3832, Crypto 4430 → 4274, RegExp 12074 → 11886, others within noise;
every fixture's score equal or higher.

Bolted static against QuickJS (`board-bolt-vs-qjs-b26.txt`, 3 runs, KiB): Richards
3216–3384 vs 3276–3480, DeltaBlue 3464 vs 3616–3928, RayTrace 3268–3456 vs 3372–3500,
NavierStokes 4388–4516 vs 4928–5080, EarleyBoyer 9832–9956 vs 16472–16640, Splay
42056–42184 vs 146884–147080; Crypto 3740–3944 vs 3516–3668 and RegExp 11388–11408 vs
6636–6848 remain. Node 22 and Bun, with and without their JITs (`board-node-bun.txt`):
every fixture at least 23 MiB (Bun without JIT), Splay 63–447 MiB.

Guard (`guard-array-payload-raw.txt`, base `origin/v2-merged` f9355e67c): per run
Crypto 0.00%, DeltaBlue +0.76%, EarleyBoyer +0.56%, NavierStokes 0.00%, RayTrace +0.06%,
RegExp +0.67%, Richards 0.00%, Splay +0.56%: the added collections. Splay's set-up runs
+144M (its extra collections, which take 6 MiB off its peak). Runtime lib 671/671,
Test262 stages 29–92 and `framework-core` 43/43 pass.

## The SDK console on the V8-v7 boards

The trunk made the global `console` Node's SDK console. Every V8-v7 fixture prints through
`console.log`, and the first call loads `console.js` and its requirements (inspect, errors,
validators, buffer, util, debuglog, diagnostics_channel, trace_events) and the primordials:
static unbolted, an empty script 3152 KiB, `console.log("x")` 12336 KiB, about 8.5 MiB of it
anonymous and resident for the rest of the run. Bolted against QuickJS
(`board-bolt-vs-qjs-b27-sdk-console.txt`): Richards 12104–12268 vs 3256–3484, Crypto
12812–12964 vs 3492–3612, Splay 50724–50752 vs 146916–147164; only Splay stays lower.

This batch keeps the load lazy (console, warning listener, process wrappers: an empty script
15.5 → 5.1 MiB on the iteration build), reserves the compiler's function slots once and drops
the parse tree before a compilation's finishing passes (Crypto 4560 → 4352 KiB unbolted,
before the SDK console), and releases read-only pages after a library loads on first use
(file pages after a console load 4084 → 1472 KiB). The console's own cost needs a decision
outside this lane (TOD-123).

## Memory-first pacing and compact program metadata (2026-10-17)

Host: Linux 6.18.44 x86_64, 4 vCPU, 16 GiB; static C runtime, BOLT-ordered build,
evicted page cache before each run, environment of 155 variables.

The trunk's Score-only collection pacing (e14aadb13: at least 2^18 cells between
collections, two to four live sets, the regex lazy DFA) took fixed-work max RSS to 17–94 MB
(Crypto 21.2, DeltaBlue 19.5, EarleyBoyer 47.6, RayTrace 17.3, RegExp 25.8, Splay 93.8 MB).
On the owner's memory-first direction, this batch restores the earlier pacing (384 cells,
1/2, 1/1, 1/1) and the literal, one-pass and backtrack regex engines. It also:
- keeps dynamic atom names once, in stable chunks (which fixes a dangling short name);
- delta-codes source positions;
- sizes register root masks to each function's registers;
- builds CommonJS wrappers at their exact length;
- lowers the complete regexp matcher on first use;
- copies `process.env` on its first read.

Bolted static against QuickJS (`board-b30.txt`, 5 runs, KiB):

| Fixture | Quench | QuickJS |
|---|---|---|
| Richards | 3000 | 3328–3432 |
| DeltaBlue | 3196–3512 | 3608–3780 |
| NavierStokes | 4664 | 4884–5016 |
| EarleyBoyer | 8904–9108 | 16540–16892 |
| Splay | 40240–40248 | 146924–147048 |
| RayTrace | 3336 | 3164–3388 (median 3348) |
| Crypto | 3592–3752 | 3444–3676 |
| RegExp | 9936–10088 | 6696–6792 |

Guard (`guard-memory-first-raw.txt`, base `origin/v2-merged` bbb9d359b, unbolted), per run:

| Fixture | Change |
|---|---|
| Crypto | −1.1% |
| NavierStokes | −2.2% |
| Richards | −2.4% |
| Splay | −0.1% |
| DeltaBlue | +5.5% |
| EarleyBoyer | +24.5% |
| RayTrace | +28.3% |
| RegExp | +65.7% |

Start-up: RegExp and Splay +41% (pacing and regex engines), the rest −1% to −4%. These
costs are the memory-first choice, not regressions in the compactions. Runtime lib 684/684,
`framework-core` 43/43, Test262 stages 29–92 (23633) and 10 (11087) pass.
