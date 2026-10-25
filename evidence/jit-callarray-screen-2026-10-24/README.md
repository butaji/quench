# Guarded compiled `CallArray` screen

This diagnostic measures commit `1f8151867995da494c434b765d28cee258d412f4`, which allows the existing guarded numeric compiled-call entry to consume the already-materialized argument values of `CallArray`. Unsupported targets, ABI guard misses, and other call forms keep the existing VM frame path.

The before binary is the parent commit `a8aa54c6a83f4cb57407ef3b647a4370a17b7b0b`; the after binary is the feature commit above. Both were built with `cargo build --locked --profile production -p quench-node --bin quench-node` and Rust 1.99.0. Their SHA-256 hashes are recorded in the raw JSON.

Host: Linux x86_64, kernel `6.18.55`, 4 logical CPUs, 4 CPU quota units, 16 GiB memory limit. Process RSS uses Linux `wait4` rusage. Node was 26.12.0 and Bun was 1.4.4. The runner's `host.rustc` field is empty, although the production binaries were built with Rust 1.99.0.

Command for each binary:

```sh
target/production/quench-bench --fixtures web-ssr,hash-map \
  --suite jetstream3 --jit --jit-js-general-dispatch --runs 3 \
  --timeout-ms 600000 --quench <binary> \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true \
  --out <report.json>
```

| Fixture | Quench before | Quench after | Node before / after | Bun before / after | Quench RSS before / after |
|---|---:|---:|---:|---:|---:|
| `hash-map` | 18.28 | 17.96 | 306.70 / 312.00 | 341.33 / 353.79 | 40.3 / 40.3 MiB |
| `web-ssr` | 16.50 | 17.45 | 95.37 / 83.66 | 113.94 / 136.28 | 117.6 / 117.6 MiB |

All four fixture/mode reports validate and produce equal output. The report marks `qualification_ready=false` and `fixture_set_matches=false`: this is a three-round two-fixture screen, not the full JetStream qualification. The peer scores vary substantially across the two short runs, and these results show no reliable end-to-end gain. Quench remains far behind both JIT peers on score; the feature did not meet the 1.20x target.

Raw reports: [before.json](before.json) and [after.json](after.json).

## Current Web-SSR JIT census

I ran a separate one-round stats-enabled diagnostic with the after binary. It
validated output but is excluded from timing comparisons. The run recorded
2,146 JS-function rows. Six dynamic eval copies of the generated 785-bytecode
`Ru` compiled GeneralDispatch: each emitted 50,016 bytes, took 21–25 ms to
compile, and entered native code about 7,760–7,800 times. Across 37 successful
compile rows, the diagnostic recorded 427,072 code bytes and 167 ms compile
time. Most dynamic eval copies remained `reclaimed`, `hot`, or
`general-compile-due`. This points to repeated dynamic code and call/frame
transitions as the next coverage question; function entry counts alone do not
measure the time saved.

Command:

```sh
target/production/quench-bench --fixtures web-ssr --suite jetstream3 --jit \
  --jit-js-general-dispatch --jit-diagnostics --runs 1 --timeout-ms 600000 \
  --quench target/quench-node-after-callarray \
  --node /workspace/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/bun-cache-1.4.4/bun-linux-x64/bun --qjs /bin/true \
  --out target/callarray-screen/web-ssr-diagnostics.json
```

Raw one-round diagnostic: [web-ssr-diagnostics.json](web-ssr-diagnostics.json).
