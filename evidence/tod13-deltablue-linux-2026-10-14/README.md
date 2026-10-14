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

### Inherited-field cache on record prototypes

`cached_inherited_field_value` and `inherited_field` walked prototypes with
`heap.get`, which only sees legacy slots. Since objects moved to record spaces,
every prototype created by `new` (DeltaBlue's `inheritsFrom` sets
`prototype = new Heir()`) failed the walk, so the deep-prototype cache never
filled and every such read took generic `get_property`. Both walks now use
`heap.ordinary_object`, which resolves record and legacy ordinary objects.
