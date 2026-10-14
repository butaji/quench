# DeltaBlue on Linux, `v2-cloud-c2` (TOD-13)

Host: Linux x86_64, 4 vCPU Intel Xeon @ 2.10GHz (KVM), 15 GiB. Engines on this
host: QuickJS 2026-06-04 (built from bellard.org tarball), Bun 1.4.2 with
`BUN_JSC_useJIT=0`, Node v26.10.0 `--jitless`.

## Baseline: `v2-merged` `f71a0ee24`, 5-round stock screen (diagnostic)

`deltablue-stock-5-base.json`, medians:

| Engine | Score | Max RSS |
| --- | ---: | ---: |
| Quench | 231 | 46,243,840 B |
| QuickJS | 901 | 3,887,104 B |
| Bun (no JIT) | 1,300 | 31,604,736 B |
| Node `--jitless` | 1,764 | 50,315,264 B |

Empty-script floor of the same binary: 43.1 MB max RSS (27.3 MB of it the VM
thread's glibc arena; see the TOD-104 finding).

## Instruction ledger (Callgrind, fixed work)

Script: `base.js` + `deltablue.js` + a runner that calls `Setup`, then `run()`
N times. Per-run cost = (Ir at N=10 − Ir at N=0) / 10.

| Binary | Ir N=0 | Ir N=10 | Per `run()` |
| --- | ---: | ---: | ---: |
| QuickJS | 8,838,711 | 705,554,737 | 69.7M |
| Quench `f71a0ee24` | 1,240,099,057 | 4,694,216,573 | 345.4M |
| + inherited-field cache on records | 1,240,197,349 | 3,993,681,626 | 275.3M (−20.3%) |
| + record receivers reach the virtual-name tiers; fast-lane reads | 1,254,243,134 | 3,444,787,137 | 219.1M (−20.4%) |

### Inherited-field cache on record prototypes

`cached_inherited_field_value` and `inherited_field` walked prototypes with
`heap.get`, which only sees legacy slots. Since objects moved to record spaces,
every prototype created by `new` (DeltaBlue's `inheritsFrom` sets
`prototype = new Heir()`) failed the walk, so the deep-prototype cache never
filled and every such read took generic `get_property`. Both walks now use
`heap.ordinary_object`, which resolves record and legacy ordinary objects.

### Virtual names on record receivers (restores `62b88162c`)

`62b88162c` let ordinary objects cache `length`, `size` and the buffer-view
names (DeltaBlue's `collection.size()` read, +28.5% then). Merge `aa407a77a`
routed record objects, and the ordinary-cell arm, through a helper that still
excluded those names for every receiver, so the read went generic again
(30.3K `size` and 18.2K `length` generic reads per run, counted with a
temporary miss log). Three changes:

- `shape_property_lookup` classifies the receiver once: object records and
  `Cell::Object` are ordinary, other cells exotic; only exotic receivers
  exclude the virtual names.
- `array.length` on the ordinary-miss arm answers from the array, as the
  ineligible arm already did (arrays are records now).
- The fast lane's `field_cache_hit` resolves its receiver through
  `shape_property_lookup`, so record receivers reach the holder and
  megamorphic tiers instead of exiting the lane.

Node oracle: `virtual-hazard.js` (ordinary, deep, array, String wrapper,
function, Map, typed array and arguments receivers on shared sites, with
prototypes that hold data `length`/`size`/`byteLength`, plus prototype
mutation after caching). Quench output equals `virtual-hazard.node26.out`
(Node v26.10.0), 67 lines. Runtime lib suite 583/583; the new test
`record_receivers_cache_virtual_names_and_inherited_fields_like_ordinary_cells`
fails without the change.

Fixed-work wall per `run()` (4 interleaved pairs, diagnostic): base
32.0–37.0 ms, candidate 19.1–20.7 ms.

### Module-level captures read their slot directly

Under `quench-node` the main CommonJS file compiles as a module-goal program,
so its top-level bindings live in the program's root environment.
`direct_capture` and `load_environment_binding` refused every root slot and
took `load_projected_environment_binding` (program lookup, global var, global
lexical and module-import checks), and the fast lane exited on each such read.
A root slot is now read directly when `ProgramStore::root_slot_is_binding`
holds: the root's variables are local or it has no global var atoms, a
`Script` root has no global lexical atoms, and the slot is not a module
import. Deleted slots (TDZ) still take the projected path.

Microbenchmark, Callgrind per iteration of `for (i = 0; i < N; i++) {}` with
`N` captured from module scope: 620 -> 370 (a local `N` is 83).

DeltaBlue, same tree with and without the change (`9d836a1eb` + change):
218.9M -> 209.8M instructions per `run()` (-4.2%).

Checks: runtime lib 583/583; Test262 stages 3 (145/145), 8 (347/347), 11
(217/217), 20 (599/599), reports `t262-cand-*.json`; Node oracle
`capture-oracle/` (ESM live import bindings read in closures, module TDZ,
CommonJS bindings, sloppy and strict eval), identical output.

Rejected: a fast-lane `LoadCapture` handler over the same `direct_capture`.
Microbenchmark 370 -> 202 per iteration, but DeltaBlue 209.8M -> 212.4M per
run (+1.2%): its lanes exit at the next dynamic `Call` anyway.

### RSS: dynamic programs share the VM's atom names

Every dynamic program (eval, `Function`, host scripts, modules) is compiled
with every VM atom as its prefix, and kept a private packed copy of all of
them. `quench-node` evaluates about 100 host scripts at startup, so these
copies grew quadratically: about 8.2 MB of live heap (massif). Once a dynamic
program's new atoms are interned its ids are the VM's, so its `AtomTable` now
becomes a view (`AtomSequence`, length) over the VM's names: the main
program's packed table plus an append-only list of the dynamic atoms' shared
host `Rc<str>`.

| Process (production build, same tree) | Max RSS before | After |
| --- | ---: | ---: |
| `quench-node` on `console.log(1)` (3 runs) | 43.0-43.1 MB | 34.7-34.8 MB |
| DeltaBlue fixed work, 40 runs | 43.9 MB | 35.5 MB |

Startup peak live heap without the record reservations, massif:
26.7 MB -> 16.4 MB.

Checks: runtime lib 584/584 (new `shared_prefixes_keep_names_across_appends`);
Test262 stages 8, 11, 20, 68, 86 all pass; stage 48 508/509, its one failure
(`built-ins/Function/call-bind-this-realm-value.js`) fails identically on the
baseline. Reports `t262-atoms-*.json`.
