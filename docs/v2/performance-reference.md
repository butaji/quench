# Performance reference points

The required parity gate is QuickJS, measured by `rqj-lab` with eleven
interleaved runs. Node/V8 is an architectural reference, not a completion gate:
this project intentionally emits no native guest code.

On the Apple M4 development host with Node 26.7.0 / V8
14.6.202.34-node.28, single-run checks of the exact bundled Richards program
produce these representative scores:

| V8 configuration | Flags | Richards Score |
|---|---|---:|
| Full tiering (Sparkplug + Maglev + TurboFan) | none | ~76,000–78,000 |
| Sparkplug disabled; Maglev + TurboFan still active | `--no-sparkplug` | ~77,000 (barely changed — Maglev/TurboFan dominate) |
| TurboFan disabled; **Maglev still active by default** | `--no-opt --no-sparkplug` | ~58,000 |
| All three JIT tiers disabled (true interpreter-only) | `--no-sparkplug --no-maglev --no-turbofan` | ~2,780 |
| Sparkplug only (Maglev/TurboFan disabled) | `--disable-optimizing-compilers` | ~4,500 |
| Jitless (no runtime executable-memory allocation) | `--jitless` | ~2,800 |
| Jitless with inline caching forced off | `--jitless --no-use-ic` | ~300 |

The same host was rechecked on 2026-09-20 with Bun 1.3.14 and the sibling
QuickJS checkout. Bun must receive the prefixed environment option
`BUN_JSC_useJIT=0`; the unprefixed `JSC_useJIT` is not the setting forwarded
by Bun. These are deliberately single-run architectural reference points, not
the eleven-run acceptance gate:

| Engine (native-code tiers disabled) | Invocation | Richards | DeltaBlue |
|---|---|---:|---:|
| V8 14.6 (Node 26.7.0) | `node --no-sparkplug --no-maglev --no-turbofan` | 2,761 | 2,916 |
| JavaScriptCore (Bun 1.3.14) | `BUN_JSC_useJIT=0 bun` | 2,743 | 2,155 |
| QuickJS (sibling checkout) | `../quickjs/qjs` | 2,091 | 1,962 |
| rqj (this checkout) | `target/release/rqj` | 2,087 | 2,234 |

The de-JITed engines therefore occupy the same small performance band on
these workloads. V8/JSC are useful aspirational ceilings, but QuickJS remains
the only Score/RSS completion gate because it is interpreter-only by design
and has a comparable process footprint.

**Correction to an earlier version of this document (superseded, was wrong on
two points):**

1. `--jitless` does **not** disable inline caching. `node --v8-options` shows
   `--use-ic` and `--lazy-feedback-allocation` default to on independently of
   `--jitless`; `--jitless` only means "disable runtime allocation of
   executable memory." Verified directly: `--jitless --no-use-ic` collapses
   the score to ~300, versus ~2,800 for `--jitless` alone — proving ICs are
   active by default under `--jitless`. The [v8.dev/blog/jitless] post
   describes a regression V8 found and *fixed* via lazy feedback-vector
   allocation during development, not the behavior of the currently shipped
   default.
2. `--no-opt --no-sparkplug` (used in the prior version of this table as "the
   fair Ignition+IC, zero-codegen comparison") is **not JIT-free**. `--no-opt`
   is an alias for `--no-turbofan`; V8 has a third tier, **Maglev**, which
   defaults to on and was never disabled by that flag combination. The
   ~58,000 score attributed to "inline caching, no code generation" was
   actually measuring Maglev-compiled code.

The corrected picture: V8's true interpreter-only number (all three JIT tiers
off) is **~2,780**, essentially identical to `--jitless`'s ~2,800 — the two
were measuring almost the same thing all along, just for different reasons
than initially assumed. This repo's interpreter currently scores ~2,061–2,111
on the same benchmark: **roughly 1.3–1.4x below true V8-interpreter-only**,
not the ~20-25x gap previously claimed. The large multi-tens-of-thousands
gap only appears once Maglev/TurboFan are included — i.e. it is, after all,
substantially explained by "V8 has a JIT and this project deliberately
doesn't," which is the simpler explanation this document originally set out
to move past. Any task or prior finding reasoning from the old ~55,000-58,000
"IC-only" number should be re-read with this correction in mind.

[Ignition's design description](https://v8.dev/blog/ignition-interpreter)
covers its compact accumulator bytecode and CodeStubAssembler-generated
handlers, reflecting years of architecture-specific tuning. A Rust `match` or
function-pointer table should retain a residual gap even after local
bytecode-density and allocation work; the ~1.3-1.4x true-interpreter gap
found above is a much smaller and more plausible target for that residual
engineering-maturity gap than the previously (incorrectly) inflated one.

Tasks 49 and 50 track an isolated feasibility spike for accumulator and dense
bytecode. Any production migration still has to beat the QuickJS Score/RSS gate
and preserve the interpreter-only integrity rules.
