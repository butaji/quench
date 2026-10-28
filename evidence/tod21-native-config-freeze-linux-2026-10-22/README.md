# `process.config` frozen natively — kept (small)

Host: Linux 6.18 x86_64 VM, 4 logical CPUs. Production profile with `profile-memory`, compile
cache on, print1. Malloc in use was read with `mallinfo2` at the checkpoints of
`meas-gc-process-split`.

## Change

`install_config` compiled `(value) => Object.freeze(value)` and called it on `variables` and
`config`. It now defines each member as `Object.freeze` leaves it (enumerable, neither writable
nor configurable) and then calls `prevent_extensions_rooted`.

## Result

The `process_config` step fell from +27.9 KiB to +21.9 KiB of malloc, and the end of
bootstrap fell from 818,304 to 812,848 bytes. Most of the step's +22 KiB is not the config
object: it is a heap growth step that happens to land there. Checkpoint deltas place growth
steps where they cross a threshold, so they are not exact attribution.

## Node oracle (`cfg.js`)

| Check | Node 22 | Quench |
| -- | -- | -- |
| `Object.isFrozen(config)` and `(config.variables)` | true, true | true, true |
| Extensible | false | false |
| Members | `writable: false, enumerable: true, configurable: false` | same |
| `variables` prototype | `Object.prototype` | `Object.prototype` |
| `process.config` descriptor | writable, enumerable, configurable data property | same |

Quench's variable set is deliberately smaller than Node's; this change does not alter it.

`cargo test -p quench-node --lib process`: 3 passed.
