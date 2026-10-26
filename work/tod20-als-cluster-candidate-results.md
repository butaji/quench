# TOD-20 isolated ALS and cluster candidate outcomes

Date: 2026-10-26 21:28 UTC

## Exact inputs

- Source base: `ef64d9b235962b17ab88005d79bf8439b5039208`.
- Combined tracked Rust diff SHA-256: `fabefeafc91ecb8a574c1bd192698345d88369581f762bc26d18afbaa68d76bb`.
- ALS diff SHA-256: `d86def4a9968814bdc3b69881ccaa86b8dc086de0e47cda7db9a1c4b147251d7`.
- Guarded cluster close diff SHA-256: `9cba8afd2ef0e6fa03a1eca40b00647477a02af388db4a33ae1b0443a122c09f`.
- Candidate child SHA-256: `ddeae56e33154a43ab001c8d697034e843bf0eb82111f9ee67689b640bcd7f72`.
- `run-parallel` SHA-256: `39a8b05082460a6a75901552b04e513b4ce92e4ca49bf533b9f56004ee826f6c`.
- Oracle: official Node v26.12.1 executable SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Official fixture root and complete inventory hash: `/tmp/tod20-node-v26.12.1-official-source/test/parallel`, `aa0d2d758e3809c7`; official physical `lib/` was used by the build.
- Native inputs: ICU `/tmp/tod20-icu/pkgconfig`, zlib `work/node-stable-zlib/lib/pkgconfig`, Brotli `work/node-stable-brotli/lib/pkgconfig`; release target `/tmp/tod20-v164-session-target`.

## Results and disposition

The isolated `test-stream-finished-async-local-storage.js` run remains **fail**, matching v386; it expects store value `test-context-1` but sees `undefined`.

The complete official 83-case `test-cluster-` cohort is **73 pass / 2 TAP skip / 5 fail / 3 timeout / 0 crash / 0 unclassified**. Matched v386 is 74 pass / 2 skip / 5 fail / 2 timeout. There are no fail→pass transitions; `test-cluster-primary-error.js` regresses pass→timeout, while all other statuses match. Both TAP skips have the explicit skip marker and there are no duplicate fixture rows.

This combined candidate has no attributable gain and contains one regression; neither patch is promoted. The ALS failure and cluster regression are separate source owners and need separate corrected candidates before any further family run. Exact results are preserved under `/tmp/tod20-fs-path-promise-fdclose-build/`:

- Cluster result SHA-256 `b4c9cb67b1331dd63daa288b340c8e4b91694d9d35a6a90a9a45626b3b3a2b6e`
- Cluster checkpoint SHA-256 `2e0a9c09788d7c33cc94220385caadf48696d139a5c497e9f773070a4c20d65d`
- Cluster log SHA-256 `63cde3825006afa125f60cb397fb737311d1e9ddbb9f8c1988c544ffd2807eaa`
- ALS result SHA-256 `55b28dd81c04136ec86fcc17ee6c1df8e9921707c71b18384025237b9ec46f40`
- ALS checkpoint SHA-256 `6b6b01cffbe25562a585d3d8fd78f17876ec374bb7bb1daeec31b631096f1e4e`
- ALS log SHA-256 `ea6544d89500f062e57fec606c0048fc839f243d84f060d5b26efeefb19aac72`

No focused result counts toward the complete 4,000-pass gate. The next isolated candidate is the one-line HTTP/2 `aborted` notification patch in `/tmp/tod20-socket-terminal-v386` (source diff `6145d485183975d0520c751e0e869e26b044504bb94572411dfe078d22c45d7a`); the HTTP/1 parser-finish control is a separate root.
