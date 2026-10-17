# node:zlib from Node's lib/zlib.js, 2026-10-17

Host: Linux 6.18.44 x86_64 (cloud VM), 4 vCPU Intel Xeon @ 2.10GHz, 15 GiB RAM.
Oracle: official Node v26.12.0. No C zlib or brotli is linked on this host
(`quench_node_zlib` and `quench_node_brotli` are both off).

`node:zlib` is Node's lib/zlib.js over `internalBinding('zlib')`.
- One zlib engine mirrors node_zlib.cc `ZlibContext` through the zlib C API: the linked
  Node zlib when present, libz-rs-sys otherwise.
- The binding gains `params`, `reset` and `crc32`. Brotli and zstd handles get Node's
  ResetStream semantics. A pure-Rust Brotli backend serves hosts without C brotli.
- The constants table is Node's, in Node's order, read-only.
- The old polyfill (`zlib_shared_vm`, `zlib_stream_shared_vm`, `zlib_options_shared_vm`)
  is removed.

## 300-package scenarios (60 s)

`scenarios-zlib.json.gz`: **286 behavior / 13 declarations / 0 Quench failures / 1 oracle
failure** (the musl Rollup addon also fails on glibc Node). minizlib@3.1.0 now passes; it
drives `_handle`/`_processChunk`. The previous run had 284/13/2/1.

## Upstream test/parallel test-zlib*.js (89 cases)

The old polyfill passed 34 (`upstream-test-zlib-polyfill.txt`, columns: node, quench).
Node's lib passes 76 (`upstream-test-zlib-node-lib.txt`); those 76 are now the `zlib`
profile in `node-tests/parallel.txt`.

Known gaps:
- Brotli encoding with a dictionary on hosts without C brotli: the Rust crate only has
  legacy prefix dictionaries, whose output Node rejects, so it reports
  ERR_ZLIB_DICTIONARY_LOAD_FAILED. Decoding Node's dictionary streams works.
- zlib-rs deflate bytes differ from Node's bundled zlib. Decompressed results agree.
- Spoofed `length` getters on views, some ZIP archive cases, and cases needing
  `--expose-gc`.
