# JavaScript-backed Wasm import call path

The paired 11-round reports in this directory are an exploratory subset run on
`gcc-loops-wasm` and `quicksort-wasm`. Both fixtures passed validation and
produced equal outputs. The reports are not qualification-ready: the selected
fixture set is only two of the five no-JIT joint-win cases, and the baseline
and candidate binaries were built from dirty worktrees without a recorded
dirty-tree hash.

The candidate is `ca47eed66edb1355f2808d686b532a5d5fa16e7946b39da7a29204929992774b`;
the baseline is `efaaabbd6b2a97003b1227c575392d729ec6611d99f6797a2732762a2e9614f8`.
Both reports identify source revision `3dda94daca038690b5e59498692023f945efa272`.
The candidate changes the compiled Wasm-to-JS import boundary to avoid
re-dispatching through the `Native::WasmHost` wrapper while preserving its
realm, opaque caller boundary, roots, interrupt checks, and cleanup.

| Fixture | Quench baseline median Score / RSS MiB | Candidate median Score / RSS MiB | Bun JIT | Node JIT |
| --- | ---: | ---: | ---: | ---: |
| gcc-loops-wasm | 54.61 / 48.1 | 54.11 / 47.9 | 178.10 / 54.3 | 188.33 / 92.1 |
| quicksort-wasm | 234.59 / 42.9 | 243.06 / 42.7 | 387.29 / 47.4 | 496.14 / 88.5 |

The trampoline screen has no demonstrated Score win against either peer. Its
RSS remains below both peers on these two fixtures. The candidate's small
Score changes versus the baseline are exploratory and do not isolate the
trampoline because the build inputs were dirty and the reports lack source
tree hashes.

## Emscripten memcpy specialization

The next experiment recognizes only the exact Emscripten helper body
`(dest, src, num) => HEAPU8.copyWithin(dest, src, src + num)` with an i32
triple signature and a real lexical `HEAPU8` capture. It reads the captured
view at every call so `updateMemoryViews()` after memory growth is observed.
Inlining is allowed only when the current view is a live, mutable `Uint8Array`
and `copyWithin` resolves through ordinary data properties to Quench's native
intrinsic. Accessors, proxies, replaced methods, and incompatible views use
the ordinary JS helper body. This is necessary because the helper is shared by
all `gcc-loops-wasm` instances and its captured view and method can change.

The focused regression test checks exact-body recognition, a replaced capture
view, and the custom-method fallback. Benchmark results for this experiment
check the helper on `gcc-loops-wasm`, whose generated JS declares `HEAPU8` in
the enclosing `setupModule` function. The focused test now uses a mutable
function-local `var` capture, verifies that the current view is reloaded after
replacement, and separately checks that a `with` object shadows an outer
lexical binding and stays on the ordinary JS path. Root lexical captures are
also accepted only when their slot is proved from the root binding metadata.

The production JIT run completed 11/11 valid rounds against Node v26.10.0 and
Bun 1.4.2 on the pinned JetStream corpus. All outputs matched. This one-fixture
report is a focused screen (`qualification_ready: false`), so it does not
claim a JetStream lane qualification:

| Mode | Quench Score / max RSS MiB | Bun | Node |
| --- | ---: | ---: | ---: |
| `--jit` | 54.80 / 47.98 | 180.07 / 54.60 | 184.05 / 62.43 |
| `--jit --jit-wasm-selective` | 70.88 / 47.65 | 180.46 / 53.91 | 182.10 / 62.42 |

The copy specialization retains GCC's lower max RSS, but neither mode wins on
Score. Selective tiering improves the cold-start-heavy GCC Score on this screen
while keeping the RSS edge; it remains far below both JIT peers and is not an
isolated A/B of the memcpy specialization. The raw reports are
`emscripten-memcpy-root-lexical-gcc-11-round.json` and
`emscripten-memcpy-selective-gcc-11-round.json`. Both record the dirty source
revision and binary hashes. The earlier pre-root screen is preserved in
`emscripten-memcpy-gcc-11-round.json`; it is not used for an isolated
before/after claim.
