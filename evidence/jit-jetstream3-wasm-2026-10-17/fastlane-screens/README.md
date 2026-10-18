# Wasm fast-lane JIT screen

This screen measures the compiled-Wasm dispatch change on the five JetStream
3.0 Wasm fixtures that had beaten Bun with JIT disabled on both score and peak
RSS in the earlier 11-round audit. The current reports use Quench JIT, Bun
1.4.2 JIT, and Node 26.10.0 with optimizing compilation proven by preflight.
Every fixture validated and produced equal output. Each report has one valid
round, so these results are exploratory screens, not qualification evidence.
All reports set `qualification_ready` to `false`.

| Fixture | Quench JIT Score / RSS MiB | Bun JIT | Node JIT | Result |
| --- | ---: | ---: | ---: | --- |
| HashSet | 21.00 / 89.7 | 203.00 / 55.0 | 220.46 / 126.8 | No score win; RSS beats Node |
| gcc-loops | 80.81 / 46.8 | 175.67 / 56.3 | 179.51 / 91.8 | No score win; RSS beats both |
| quicksort | 173.27 / 41.5 | 325.82 / 48.9 | 333.13 / 88.1 | No score win; RSS beats both |
| tsf | 10.24 / 224.5 | 187.17 / 131.8 | 172.29 / 146.1 | No win on either metric |
| zlib | 13.93 / 112.6 | 85.94 / 100.0 | 87.07 / 113.2 | No score win; RSS beats Node |

The requested score-and-RSS wins were not achieved. Compared with the earlier
11-round Quench no-JIT results, JIT raises the one-round score screen for
gcc-loops, quicksort, and zlib, but remains below both JIT peers. The sizeable
gap is consistent with substantial work staying in interpreted JavaScript or
unsupported Wasm functions; this screen alone does not isolate their shares.

## Native-entry diagnostics

These separate diagnostic runs enable `--jit-diagnostics`; their timing and
RSS must not be compared as clean scores. The counters come from the raw
`QUENCH_WASM_JIT_STATS` line in each Quench sample's stderr. Function IDs are
zero-based defined-function indices; adding the module's import count maps
them to Emscripten's `.js.symbols` entry. The Wasm name section is stripped, so
the symbol sidecars provide the function names shown here.

| Fixture | Compiled / total | Compile ms | Code / mapped KiB | Most-entered native functions | Most-entered fallback functions | VM callouts |
| --- | ---: | ---: | ---: | --- | --- | ---: |
| HashSet | 18 / 35 | 40.6 | 12.2 / 16 | `emscripten_builtin_free`: 9.97M, `__memset`: 6.15M | `HashTable<unsigned int>::add`: 39.53M, `HashTable<Node*>::add`: 9.49M, `malloc`: 8.37M | 100 |
| gcc-loops | 317 / 641 | 115.8 | 162.6 / 164 | `example1`: 512k, `example23`: 410k, `example2a`: 205k | `__fwritex`: 4.95k, `pad`: 3.80k, `out`: 2.95k | 309.1k |
| quicksort | 13 / 20 | 4.6 | 6.4 / 8 | `getint`: 10k, `Quicksort`: 5k, `__memset`: 5k | `__fwritex`: 20k, `out`: 20k, `pad`: 20k | 0 |
| tsf | 77 / 244 | 31.4 | 48.7 / 52 | `__memcpy`: 26.89M, `strlen`: 6.93M, `__memset`: 5.46M | `__fwritex`: 10.78M, `out`: 10.78M, `pad`: 10.65M | 22.4k |
| zlib | 33 / 47 | 46.7 | 76.2 / 80 | `longest_match`: 7.70M, `fill_window`: 65.2k, `strchr`: 9.6k | `malloc`: 4.52k, `__fwritex`: 4.00k, `fwrite`: 4.00k | 6.0k |

Peak process RSS is much larger than the executable mappings in every fixture.
That rules out mapped code size as the sole memory target; heap, runtime state,
and temporary compilation allocations still need separate attribution. The
fallback counters show concentrated interpreted work in HashSet's two
`HashTable::add` functions and TSF's stdio formatting path. Opt-in first
rejection locations are being added to the next diagnostic pass so an opcode
extension can target those fallbacks without broad speculative changes.

## Reproduction

The binary was built from source revision `e64538e072a89c6b0fe7b85904565ff332fd5acf`
with the production profile. Each command used the pinned local comparator
binaries and a 600-second per-process timeout. The diagnostic and clean runs
were executed serially.

```sh
for fixture in HashSet-wasm gcc-loops-wasm quicksort-wasm tsf-wasm zlib-wasm; do
  target/production/quench-bench "$fixture" --suite jetstream3 --jit --runs 1 \
    --timeout-ms 600000 --quench target/production/quench-node --qjs /bin/false \
    --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
    --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
    --out "evidence/jit-jetstream3-wasm-2026-10-17/fastlane-screens/$fixture.json"
done
```

The diagnostic reports use the same command with `--jit-diagnostics` and the
output directory `native-diagnostics-fastlane/`. Raw reports preserve engine
proofs, command lines, output, host samples, compile counters, and the
qualification flag.
