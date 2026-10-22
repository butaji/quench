# JIT opcode-denotation routing screen

This diagnostic screen checks behavior after commit `6ad0c643d2dbd14cdc2b64092324ab012b8ae8b2`, which routes the existing straight-line numeric Cranelift lowering through the opcode macro's generated JS denotation table.

## Reproduction

```sh
CARGO_BUILD_JOBS=2 cargo build --locked --profile production -p quench-node -p quench-bench
target/production/quench-bench --fixtures typescript-lib --suite jetstream3 --jit --runs 3 --timeout-ms 600000 --quench target/production/quench-node --node node --bun /path/to/bun --checkpoint /tmp/jit-denotation-routing.checkpoint.json --out /tmp/jit-denotation-routing.json
```

The report was produced with a clean source tree on Linux x86_64, Node v24.19.0, and Bun 1.4.3 with `BUN_JSC_useJIT=1`. Its source revision is the commit above, and `source_dirty` is false. The runner validated all three rounds and reported equal output.

## Result

| Engine | Median JetStream Score | Median maximum RSS |
| --- | ---: | ---: |
| Quench `--jit` | 0.15 | 187.2 MiB |
| Bun JIT | 1.72 | 350.0 MiB |
| Node JIT | 2.08 | 346.9 MiB |

Quench scored about 0.087x Bun and 0.072x Node. RSS was lower than both. This is a three-round screen, below the 11-round qualification gate, and it establishes no performance win. The change was expected to preserve the current lowering behavior; the purpose of this screen is to catch a material regression while wiring generated opcode metadata into code generation.

The raw runner output is `typescript-lib.json`. The runtime library test build was attempted with the requested iteration settings, but rustc was SIGKILLed at the 16 GiB cgroup limit before tests ran. The iteration-profile `cargo check --lib --tests` and production build passed. A separate hot-function JIT smoke matched interpreter and Node output and recorded 369 native entries from 400 calls.

## One-round JIT diagnostics

`typescript-lib-diagnostic.json` is a separate diagnostic-only run on the same committed implementation (the report's source tree was clean). It is not a timing comparison. It recorded 1,275 function rows: 28 compiled functions with 11,736,507 native entries from 11,737,275 entries, 114,688 code bytes, and 16.6 ms cumulative compile time. Another 228 functions were reclaimed after 3,722,323 interpreted entries, while 149 were marked unsupported after 20,519 entries. The diagnostic run's one-round score is instrumented and must not be compared with the uninstrumented screen.

The hottest inclusive interpreter rows include `__webpack_require__` (206.7 ms), `resolveAnonymousTypeMembers` (59.4 ms, lane-only, with 4,031 general-dispatch Unary operations), `checkElementAccessExpression` (58.2 ms, lane-only, with 96 general-dispatch Calls), and `getJSDocTagsWorker` (15.6 ms, with 13,626 general-dispatch SetFieldStrict and 3,942 GetField operations). Inclusive timings overlap nested callees, and bytecode/general-dispatch counts are not exclusive CPU time. These data point toward generic call/capture and object-write coverage plus admission/reclamation payback as the next experiments; they do not show those paths will win.
