# Closure retention: what a closure keeps alive

The question was: what does this keep alive, and for how long?

```js
function heavyTask() {
  const massiveData = new Array(1_000_000).fill("data");
  return function () { console.log(massiveData.length); };
}
const runTask = heavyTask();
```

The closure must keep `massiveData` while `runTask` is reachable, and nothing it cannot
read. Once `runTask` is unreachable, a collection must free the array.

## Host and builds

- Host: Linux x86_64 cloud guest (kernel 6.18.44), 4 vCPU Intel Xeon at 2.80 GHz, 15.7 GiB
  RAM, no swap. Other work shared the host and the disk.
- Quench: `quench-node`, `iteration` profile (opt-level 1, no LTO), at `v2-gc` 1ad28ed627
  ("before") and at this branch ("after"). These are diagnostic builds, not the production
  profile.
- Node: `/opt/node22/bin/node` v22.22.0. QuickJS (`qjs`) is not installed on this host, so
  there is no QuickJS column.

`measure.sh ENGINE LABEL` runs every script below with one engine and writes
`<script>-<label>.txt`. Run it from this directory; the scripts are CommonJS (`.cjs`)
because the repository's `package.json` makes `.js` files modules.

## 1. Retention by pattern (`probe.cjs`)

Each case keeps one closure and holds `WeakRef`s to objects it must or must not keep. The
script runs `gc()` twice in a later macrotask, then drops the closures and collects again.
"Kept" means the object survived while its closure was alive but the closure cannot read
it. Results are in `probe-*.txt`.

| case | Node | Quench before | Quench after |
| -- | -- | -- | -- |
| user example: `massiveData` while `runTask` lives | kept (required) | kept | kept |
| binding captured only by a dead sibling closure | **kept** | **kept** | freed |
| local no closure captures | freed | freed | freed |
| parameter no closure captures | freed | freed | freed |
| `arguments` captured only by a dead sibling arrow | **kept** | **kept** | freed |
| `arguments` read by the closure | kept (required) | kept | kept |
| receiver of an arrow that never reads `this` | freed | **kept** | **kept** |
| receiver read only by a dead sibling arrow (closure is a non-arrow) | **kept** | **kept** | freed |
| receiver read by the closure (`() => this.tag`) | kept (required) | kept | kept |
| block binding of a dead sibling | freed | **kept** | freed |
| loop iteration bindings of dead closures | freed | **kept** | freed |
| other iterations, beside the kept iteration's closure | freed | **kept** | freed |
| catch parameter of a dead closure | freed | **kept** | freed |
| generator local, closure yielded | freed | **kept** | freed |
| generator local, generator finished | freed | **kept** | freed |
| async function local | freed | **kept** | freed |
| class constructor local | freed | **kept** | freed |
| class method local | freed | freed | freed |
| direct `eval` in the closure | kept (required) | kept | kept |
| `with` in the closure | kept (required) | kept | kept |
| over-retained cases | 3 | 13 | 1 |

After the closures are dropped, every probed object is freed by all three engines (0 of 25
alive). Neither engine leaks. Before the fix, Quench retained more than V8: 13 cases where
a closure kept something it cannot read, against V8's 3, which are V8's shared-context leak.
After the fix Quench keeps one: an arrow that never reads `this` still keeps its receiver
(see "Open").

## 2. Representation and RSS of the user's example (`rss.cjs`)

Three runs each; `rss-*.txt` has every sample. The heap figures come from
`process.memoryUsage().heapUsed` after two `gc()` calls. Peak is VmHWM from `/proc/self/status`.

| | Node | Quench before | Quench after |
| -- | -- | -- | -- |
| `new Array(1e6)`: heap bytes per element | 8.19 | 0 (length only, no storage) | 0 |
| after `.fill("data")`: heap bytes per element | 8.00 | 8.00 | 8.00 |
| after `.fill`: RSS bytes per element | 8.2–8.3 | 8.4–8.5 | 8.4–8.5 |
| `heavyTask()` time | 6.5–7.2 ms | 310–333 ms | 5.2–6.7 ms |
| peak RSS rise during `heavyTask()` | +7.8–7.9 MiB | +20.1–21.4 MiB | +10.4–11.7 MiB |
| heap after `runTask = null` and a collection, against before the call | −7.6 MiB¹ | +1 KiB | +1 KiB |

¹ Node also frees the script's unused `new Array(N)`, which Quench's top-level binding keeps.

Both engines store the filled array densely: one 8-byte value per element, all naming the
same string. Neither copies the string and neither falls back to holey or sparse storage
after `fill`. Before the fix, Quench's `fill` ran one generic `[[Set]]` per index, and a
length-only array takes those through its sparse table first. That cost about 45× Node's
time and peaked at 2.5× the array's size. The array and its storage are freed by the next
full collection: after one `gc()` the heap is back at its pre-call level (+1 KiB) and RSS is
within 0.7–0.8 MiB of it (`residualRssKiB`), before and after the fix alike.

## 3. Collection cost (`gc_cost.cjs`)

This is the adversarial case for the new trace: 200,000 live closures, each with its own
environment of three bindings, of which it reads one; a dead sibling read the other two.
The figure is the median of 15 forced full collections. Runs are interleaved before/after;
see `gc-cost-ab-200k.txt` and `gc-cost-scaling.txt`.

| live closures | before: ms per full GC / live heap | after: ms per full GC / live heap |
| -- | -- | -- |
| 25,000 | 5.9 / 5.1 MiB | 6.4 / 3.4 MiB |
| 50,000 | 11.4 / 9.8 MiB | 15.4 / 6.3 MiB |
| 100,000 | 22.8 / 19.1 MiB | 29.7 / 12.2 MiB |
| 200,000 (5 rounds) | 45.2–47.7 / 37.6 MiB | 57.6–61.2 / 23.9 MiB |
| Node, 200,000 | 30.7–35.1 / 50–59 MiB | |

The trace keeps 36% less live heap here, and a full collection takes about 25% longer: each
partly read environment costs a record while marking and a pass when clearing. An
environment read whole costs nothing extra. The first implementation used a hash map and
was 3.5× slower at 200,000 closures (164 ms), because its probes and clearing touched the
heap in hash order. It was replaced by a dense index by heap slot, allocated lazily.

## 4. V8-v7 diagnostic (`v8_diag.py`)

Two runs, each three interleaved rounds per fixture, before against after, from iteration
builds. `v8-v7-diag.txt` is the final build. `v8-v7-diag-eager-index.txt` is the build just
before it, whose trace index was allocated eagerly. Every run of all eight fixtures completed
with a valid Score. The fixtures check their own results, so these runs also stress the
cleared environments under real collections. Median Scores, after against before:

| fixture | first run | final run |
| -- | -- | -- |
| richards | +5.6% | −4.6% |
| deltablue | −5.7% | −4.3% |
| crypto | −2.6% | +0.3% |
| raytrace | +4.0% | +1.0% |
| earley-boyer | +1.8% | −1.0% |
| regexp | −5.0% | −5.6% |
| splay | −2.0% | +3.4% |
| navier-stokes | +4.0% | +3.4% |

Most differences change sign between runs; deltablue and regexp are lower in both, by 4–6%.
Three rounds on this shared host cannot separate that from noise. A production-profile
qualification (11 rounds) is needed before any claim either way. The RSS column is the
launched process's `ru_maxrss` from `wait4`. It reads about 12.7 MiB for most fixtures, so
it was not cross-checked with `quench-bench`'s `rss-exec`; treat it as unverified. This
evidence makes no performance claim.

## What changed

- **Root cause (over-retention):** every closure an activation makes shares one environment
  record, and a full collection traced a closure's environment edge whole: every slot, every
  enclosing environment and its lexical `this`. Generators, async functions, class
  constructors and functions enclosing a direct `eval` copy every local into that
  environment, so their uncaptured locals were kept too. Block, loop and catch bindings live
  in the same record, so a closure in a block kept its siblings' bindings.
- **Fix:** `bytecode::ClosureReads`, derived per program from the compiler's capture facts.
  It records what each function and the functions nested in it can read of the environment
  each distance out: captures, by-name slots (home objects, declared compiler bindings), a
  constructor's instance-initializer reads, and an arrow's lexical `this` up to the first
  non-arrow. A full collection traces closure edges through it (`heap::EnvironmentTrace`) and
  clears every slot no reader reads. Dynamic scopes, program roots and any layout the table
  does not describe are read whole, so the guard falls back to the old trace. The table is
  part of the program encoding, so cached programs trace the same way. The contract is in
  `docs/v2/closure-environments.md`.
- **A second retention found by the suite:** a script's root slot for a global `var`
  "projects" the global object. Reads go through it, yet it kept a copy of the root code's
  last write. Any top-level function keeps the root environment alive, so a global
  reassigned from inside a function stayed alive at its old value. Every reader goes
  through the slot to the global object, so the trace keeps only the slot's initialization
  state.
- **`fill`:** `Array.prototype.fill` on an extensible array without index descriptors, whose
  prototypes claim no index, stores the range in one pass (`Heap::fill_array_elements`). A
  length-only array turns dense first when the filled range makes it dense enough, by the
  same rule `sparse_set` applies. `fill-*.txt` compares 21 edge cases with Node: prototype
  setters, read-only prototype indices, frozen, sealed and non-extensible arrays, read-only
  and accessor elements, subclasses, `arguments`, array-likes, and coercion that mutates the
  array. All match.

## Tests

`crates/quench-runtime/src/vm/closure_retention_tests.rs`: 28 cases plus a completeness
check. Each case runs under the specialized and unspecialized compilations, once and again
after 2000 runs, which tiers it up (`LOOP_TIER_THRESHOLD` × 2). Each case asserts:

- what stays alive and what is freed while the closure lives;
- that the closure still runs after the collection cleared what it cannot read;
- that everything is freed once it is dropped.

`bytecode::closure_reads` has unit tests for the derivation, and `bytecode/tests.rs` checks
the encoding round trip. `cargo test --profile iteration -p quench-runtime --lib`: 882
passed, 0 failed. The first full run caught a class field initializer whose reads the
constructor's demand lacked; that led to the instance-initializer rule above.

## Open

- An arrow that never reads `this` keeps its defining activation's receiver. No compiler
  fact says which arrows read `this` (`LoadThis`, `super`, field operations with a `this`
  base, …), so every arrow is assumed to. Node frees it.
- Minor collections, and heaps without function records (which mark in parallel), trace
  environments whole. The fix applies at full collections.
- A released class constructor (`finish_early`) does not carry its instance-initializer link
  at program assembly, so its closures read their chain whole.
- The `debug_verify_full` path, which reruns marking with a fresh trace, is not exercised by
  the `iteration` profile, which has no debug assertions.
- No production-profile V8-v7 qualification was run: the host did not have the disk space
  for a production build next to this work.

## Production qualification on V8-v7: not merged yet

Setup:
- Both sides are static production builds with the same flags, unbolted: `qvb` is v2-gc
  `053d759ab6`; `qcr` is this branch merged with it (`e785823816`).
- Binaries are evicted before each run, and the A/B pairs alternate.
- Raw results: `v8v7-production-memory-first-7-runs.txt` and `v8v7-production-throughput-5-runs.txt`.
- This host's A/A floor is about ±130 KiB.

Results:
- **MemoryFirst:** median max RSS rises on 12 of 16 rows, mostly by +80 to +204 KiB, against
  falls on 4: DeltaBlue −84, DeltaBlue `--jit` −36, NavierStokes `--jit` −184, Richards
  `--jit` −4.
- **Throughput:** most rows are within ±140 KiB, but Crypto `--jit` is +460 and EarleyBoyer
  `--jit` +412.
- **Score:** within noise except Splay under MemoryFirst, at ×0.91 and ×0.93.

V8-v7 has few over-captured environments, so it pays for the readable-slot table and the
larger code without gaining from them. Merging waits on cutting that cost: build the table
only for programs with shared closure environments, or derive it lazily at the first full
collection that traces one. Then rerun this A/B.

## After gating the trim and indexing traced environments sparsely

Commit: `Trim closure environments only as the live set grows; index traced environments sparsely`.

- **Unbolted, against v2-gc** (`v8v7-gated-unbolted-*`):
  - MemoryFirst medians average about +40 KiB, with rows from −172 to +152.
  - Throughput is mixed, apart from one EarleyBoyer +540 sample.
- **Bolted, each binary with a BOLT profile retrained on itself**
  (`v8v7-gated-bolted-retrained-memory-first-7-runs.txt`): +68 to +232 KiB on 15 of 16 rows,
  so the cost is real. Score is within noise.
- **Trim on against trim off, in one binary** (`trim-on-off-same-binary-7-runs.txt`): the
  trim itself accounts for at most about 100 KiB (RayTrace, RegExp), and 0 on most rows.
- **Sampled `/proc` peaks** differ by only 20–40 KiB, while the kernel's max RSS differs more.
  The rest of the cost is therefore a short spike that the sampler misses, and it has not been
  located yet.

Status: the branch stays unmerged. It is semantically ready (885/885 tests), but on V8-v7 it
costs 70–230 KiB of max RSS.
