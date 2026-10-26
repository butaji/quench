# TOD-20 isolated ALPN callback candidate: focused outcome

Date: 2026-10-26 21:11 UTC

## Source and binary

- Candidate base: `ef64d9b235962b17ab88005d79bf8439b5039208`.
- Five-file tracked Rust diff SHA-256 (before build): `c3dca271d1774d153ec2bc7fa1bf0f5255cb9caf4e8eeb6dbe59d5bc30c86c4d`.
- Pinned release child SHA-256: `9ad4d21134799511bcade657c869a2e73755b77607669e7979da6eea38a9925b`.
- Runner SHA-256: `39a8b05082460a6a75901552b04e513b4ce92e4ca49bf533b9f56004ee826f6c`.
- Oracle: official Node v26.12.1 executable SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Fixture root: physical official Node v26.12.1 source at `/tmp/tod20-node-v26.12.1-official-source`, inventory hash `aa0d2d758e3809c7`.
- The temporary build root used official Node v26.12.1 `lib/`; `src/` and `deps/` were linked from pinned submodule commit `26e72d85a611e222e3f06025de8a3c496923bacd`. `src/node_version.h` SHA-256 was `971263ecd7bf9534c80ab1c69bf4df1643d5c9af1a913bcdea85d6b39589c92c`.
- Native pkg-config inputs: ICU `/tmp/tod20-icu/pkgconfig`, zlib `work/node-stable-zlib/lib/pkgconfig`, Brotli `work/node-stable-brotli/lib/pkgconfig`; release target `/tmp/tod20-v164-session-target`.

## Official focused results

`run-parallel --all --parallel-dir /tmp/tod20-node-v26.12.1-official-source/test/parallel --filter test-tls-alpn --jobs 1 --timeout-secs 60` completed five fixtures: **1 pass / 0 skip / 4 fail / 0 timeout / 0 crash / 0 unclassified**. The four callback cases still negotiated `false` instead of the expected ALPN protocol; `test-tls-alpn-protocols-validation.js` passed. The matched v386 baseline for these same five fixtures is identical (1/0/4/0/0/0), so the candidate has no attributable gain or regression and is not promotable.

The complete result, JSONL checkpoint and log are preserved under `/tmp/tod20-alpn-callback-v386/` with SHA-256 values:

- `tls-focused-results.json`: `b3140bfd7a3bff8be68f57dc5cf703b6d70d15de3ea20b45ad51067ab0ef3f9a`
- `tls-focused-checkpoint.jsonl`: `c3a849ce03d3506435ba9e4ab14c723f42c09423ee033cebcc0627ec2084c679`
- `tls-focused.log`: `c0268e042dfea4ed2bebdaeaa72d374b1b8cb21a5f506c24d689c00315bcb55e`

The pinned Node v26.12.1 custom mutable-protocol oracle passed: the server changes the callback-visible first protocol from `a` to `zz` and returns it, while Node still negotiates original wire protocol `a`. This does not qualify the Quench candidate, whose official callback cases above remain unchanged. Source and compile probe history is in `work/tod20-alpn-build-compile-failure.md`.

## Disposition

Keep the candidate isolated. Trace whether the owner-thread ALPN request is created, invoked, completed, and applied to the second OpenSSL handshake; only after the first missing transition is fixed should another build run. Do not count the custom oracle or focused results toward the full 4,000-pass gate.
