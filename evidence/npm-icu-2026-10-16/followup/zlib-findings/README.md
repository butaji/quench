# Synchronous compression investigation (not integrated)

Current frozen production source 383fcec (CLI6488fa) lacks _processChunk and emits gzip OS byte ff where Node26.12 emits03 on Linux. Current public process.versions.zlib is hardcoded1.3.0; oracle reports1.3.2.1-motley-456ae73.

The existing Rust flate2/zlib-rs backend differs on90/600 exact streaming compression cases. Another150 Z_BLOCK cases are excluded from that prototype because FlushCompress does not expose Block; this is an explicit gap, not a pass. The first compile failure is preserved.

A separate Rust FFI streaming probe using the exact pinned Node deps/zlib sources matches750/750 cases (Deflate,DeflateRaw,Gzip;levels0..9;None,Partial,Sync,Full,Block flushes followed byFinish;empty,small,byte-range,repeated and seeded random buffers). Pinned-finding.json, raw JSONL and native-main.rs bind this bounded result. It is not production parity and does not cover decompression,dictionaries,reset/params,error paths,async scheduling,metadata,Brotli orZstd.

build-pinned.py derives Linuxx64 defines/source choices from Nodezlib.gyp/BUILD.gn at26e72d85 and adds Z_PREFIX to isolate symbols; output /workspace/shared/node-zlib-draft. The handle is Box-owned so z_stream addresses remain stable. A later production repair must preserve generic codec/handle lifecycle and Node close/monkeypatch behavior, not add a package-specific path or inflate results from only round trips.
