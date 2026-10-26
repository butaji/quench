# TOD-20 ALPN candidate compile probe

Date: 2026-10-26 21:03 UTC

The isolated ALPN callback candidate at `/tmp/tod20-alpn-callback-v386` was
based on frozen source `ef64d9b235962b17ab88005d79bf8439b5039208`. Its revised
source diff SHA-256 was
`26ff9ebe04bbc7ef7c587f22419a810415487f892ed00adb9bbe80c2378ee6ff`.
The pinned Node v26.12.1 mutable-protocol oracle passed when invoked as CommonJS
stdin with `/workspace/work/node-v26.12.1-linux-x64/bin/node`.

The first two build attempts exited before compilation because the sparse
candidate worktree lacked Cargo workspace members and patched vendor paths.
After adding the required sparse paths, the build reached `quench-node` but
failed. The first attempt lacked the ICU `PKG_CONFIG_PATH`, so the ICU-gated
`text_decoder_icu` errors are environment errors and must be removed on retry.
The candidate also has four real Rust API errors in
`modules/http_shared_vm/poll.rs`: it calls `evaluate_script_rooted` and
`undefined` on `&mut Runtime<NodeHost>`, which exposes neither method. No
candidate binary was produced and no ALPN fixture result is attributable.

The build used target `/tmp/tod20-v164-session-target`; `tests/node` was a
temporary hybrid root with official v26.12.1 `lib/` and the pinned source
submodule's `src/` and `deps/`. The source header SHA-256 was
`971263ecd7bf9534c80ab1c69bf4df1643d5c9af1a913bcdea85d6b39589c92c`.
The next attempt must set the pinned ICU, zlib and Brotli pkg-config paths,
fix rooted runtime access in the candidate, and preserve the candidate's exact
Node-library inputs. `/tmp` headroom after the failed compile was 384 MiB; no
build process remains live.
