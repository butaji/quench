# Selective GCC-Wasm import-callout attribution

This is a one-round, diagnostic-only run of JetStream 3 `gcc-loops-wasm` on
the clean source revision `0f5fb6579cd4d6d6602a0de27dac4631a6814fa5`. The
selective Wasm tier was enabled, JIT diagnostics were enabled, and the fixture
validated with equal output. The report is not qualification or performance
evidence.

Across 75 compiled artifacts, the report attributes 309,039 compiled import
callouts:

| Wasm function index | Import declaration index | Import | Callouts |
| ---: | ---: | --- | ---: |
| 1 | 1 | `env._emscripten_memcpy_js` | 307,270 |
| 2 | 2 | `env.emscripten_date_now` | 1,769 |

For every artifact, the attributed symbol counts sum exactly to
`compiled_import_callouts`. Function index and original import declaration
index are both included because non-function imports can interleave with
function imports in a module's import section. The counter runs at the VM
boundary, so it includes early-import hits and excludes generated native-leaf
hits that bypass `import_callout` (reported separately).

This confirms the memcpy import dominates the boundary traffic, but does not
justify another import-specific fast path. The earlier early-import A/B lost
Score, and the native memcpy-leaf A/B produced zero leaf hits; see
`evidence/jit-jetstream3-wasm-2026-10-18/early-emscripten-copy/` and
`evidence/jit-jetstream3-wasm-2026-10-18/memcpy-native-leaf/`. Further work
should profile compiled loop execution and call-site cost before changing this
boundary.

Reproduce with the production binaries and pinned peers:

```sh
QUENCH_WASM_JIT_STATS=1 target/production/quench-bench \
  --fixtures gcc-loops-wasm --suite jetstream3 --jit \
  --jit-wasm-selective --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-19/import-callout-attribution/selective-one-round.json
```

Executable SHA-256: `quench-bench` `201f2aab00fcaa783c86807ce3a30d5450a3128e78e55599d44e67a3adec4406`,
`quench-node` `c44ee4eea5ab1b52b381f98cc6dbfdcd15cd4e86bb87087fceace76769f72e92`.
The raw report SHA-256 is `e139d9a8db3545d49d820b0c62f1ede894fc06932add24ebb743d9327b4af97f`.
